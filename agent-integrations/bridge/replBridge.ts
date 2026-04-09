import { randomUUID } from "node:crypto";

import {
  BoundedUUIDSet,
  extractTitleText,
  isBridgeChatMessage,
  isBridgeControlRequest,
  isBridgeControlResponse,
  isBridgeResultMessage,
  isEligibleBridgeMessage,
  makeControlError,
  makeControlSuccess,
  makeInitializeResponse,
  makeResultMessage,
  parseBridgePayload,
} from "./messaging.js";
import { createSessionRunner } from "./sessionRunner.js";
import type {
  BridgeChatMessage,
  BridgeControlRequest,
  BridgeControlResponse,
  BridgePermissionBroker,
  BridgeState,
  BridgeTransport,
  BridgeWireMessage,
  ReplBridgeHandle,
  ReplBridgeOptions,
} from "./types.js";

export class ReplBridgeController implements ReplBridgeHandle {
  readonly bridgeSessionId: string;
  readonly sessionId: string;

  private readonly permissionBroker?: BridgePermissionBroker;
  private readonly permissionTimeoutMs: number;
  private readonly outboundOnly: boolean;
  private readonly recentPostedUUIDs = new BoundedUUIDSet(1024);
  private readonly recentInboundUUIDs = new BoundedUUIDSet(1024);
  private readonly sessionHandle;
  private transport: BridgeTransport;
  private state: BridgeState = "idle";
  private consumeGeneration = 0;
  private closed = false;
  private userMessageHookDone = false;

  constructor(private readonly options: ReplBridgeOptions) {
    this.sessionId = options.sessionId;
    this.bridgeSessionId = options.sessionId;
    this.transport = options.transport;
    this.permissionBroker = options.permissionBroker;
    this.permissionTimeoutMs = options.permissionTimeoutMs ?? 60_000;
    this.outboundOnly = options.outboundOnly ?? false;
    const sessionRunner = options.sessionRunner ?? createSessionRunner();
    this.sessionHandle = sessionRunner.spawn({
      sessionId: options.sessionId,
      accessToken: options.accessToken,
      onActivity: () => {},
      onInboundMessage: options.onInboundMessage,
      onControlResponse: options.onPermissionResponse,
    });
  }

  async start(): Promise<void> {
    await this.attachTransport(this.transport, false);
    if (this.options.initialMessages?.length) {
      await this.writeMessages(this.options.initialMessages);
    }
  }

  getState(): BridgeState {
    return this.state;
  }

  getSessionHandle() {
    return this.sessionHandle;
  }

  async writeMessages(messages: BridgeChatMessage[]): Promise<void> {
    const payloads: BridgeChatMessage[] = [];
    for (const message of messages) {
      if (!isEligibleBridgeMessage(message)) {
        continue;
      }
      const outbound = this.prepareOutboundMessage(message);
      payloads.push(outbound);
      this.sessionHandle.writeOutbound(outbound);
      if (!this.userMessageHookDone) {
        const titleText = extractTitleText(outbound);
        if (titleText) {
          const done = this.options.onUserMessage?.(titleText, this.sessionId);
          this.userMessageHookDone = Boolean(done);
        }
      }
    }
    if (payloads.length === 0) {
      return;
    }
    await this.transport.sendBatch(payloads);
  }

  async writeSdkMessages(messages: BridgeWireMessage[]): Promise<void> {
    const prepared = messages.map(message => this.prepareWireMessage(message));
    await this.transport.sendBatch(prepared);
  }

  async sendControlRequest(request: BridgeControlRequest): Promise<void> {
    await this.transport.send(this.prepareWireMessage(request));
  }

  async sendControlResponse(response: BridgeControlResponse): Promise<void> {
    const prepared = this.prepareWireMessage(response) as BridgeControlResponse;
    this.sessionHandle.writeControlResponse(prepared);
    await this.transport.send(prepared);
  }

  sendControlCancelRequest(requestId: string): void {
    this.permissionBroker?.cancelRequest(requestId);
  }

  async sendResult(result = ""): Promise<void> {
    const message = makeResultMessage(this.sessionId, result);
    await this.transport.send(message);
    this.sessionHandle.complete("completed", result);
  }

  async replaceTransport(transport: BridgeTransport): Promise<void> {
    await this.attachTransport(transport, true);
  }

  async teardown(): Promise<void> {
    if (this.closed) {
      return;
    }
    this.closed = true;
    if (this.getState() !== "failed") {
      try {
        await this.sendResult();
      } catch {
        // Ignore result-send errors during teardown.
      }
    }
    this.consumeGeneration += 1;
    await this.transport.close(1000, "bridge teardown");
    this.setState("closed");
  }

  private async attachTransport(
    transport: BridgeTransport,
    replacing: boolean,
  ): Promise<void> {
    const previous = this.transport;
    this.transport = transport;
    this.consumeGeneration += 1;
    const generation = this.consumeGeneration;
    this.setState(replacing ? "reconnecting" : "connecting");
    await transport.connect();
    if (this.closed || generation !== this.consumeGeneration) {
      await transport.close(1000, "stale bridge transport");
      return;
    }
    this.setState("connected");
    void this.consumeTransport(transport, generation);
    if (replacing && previous !== transport) {
      await previous.close(1000, "bridge transport replaced");
    }
  }

  private async consumeTransport(
    transport: BridgeTransport,
    generation: number,
  ): Promise<void> {
    try {
      for await (const raw of transport) {
        if (this.closed || generation !== this.consumeGeneration) {
          break;
        }
        const parsed = parseBridgePayload(raw);
        if (!parsed) {
          continue;
        }

        if (isBridgeControlRequest(parsed)) {
          if (parsed.request.subtype === "can_use_tool") {
            void this.handlePermissionRequest(parsed);
          } else {
            const response = this.respondToControlRequest(parsed);
            if (response) {
              await this.sendControlResponse(response);
            }
          }
          continue;
        }

        if (isBridgeControlResponse(parsed)) {
          this.sessionHandle.writeControlResponse(parsed);
          continue;
        }

        if (isBridgeChatMessage(parsed)) {
          const uuid = parsed.uuid;
          if (uuid && this.recentPostedUUIDs.has(uuid)) {
            continue;
          }
          if (uuid && this.recentInboundUUIDs.has(uuid)) {
            continue;
          }
          if (uuid) {
            this.recentInboundUUIDs.add(uuid);
          }
          await this.sessionHandle.writeInbound(parsed);
          continue;
        }

        if (isBridgeResultMessage(parsed)) {
          if (parsed.subtype === "error") {
            this.sessionHandle.fail(parsed.errors?.[0] ?? parsed.result);
            this.setState("failed", parsed.result);
          } else {
            this.sessionHandle.complete("completed", parsed.result);
          }
        }
      }
    } catch (error) {
      if (!this.closed && generation === this.consumeGeneration) {
        const message = formatError(error);
        this.sessionHandle.fail(message);
        this.setState("failed", message);
      }
    }
  }

  private respondToControlRequest(
    request: BridgeControlRequest,
  ): BridgeControlResponse | null {
    const subtype = request.request.subtype;

    if (this.outboundOnly && subtype !== "initialize") {
      return makeControlError(
        request.request_id,
        "This bridge is outbound-only and cannot accept remote control requests.",
        this.sessionId,
      );
    }

    switch (subtype) {
      case "initialize":
        return makeInitializeResponse(request.request_id, this.sessionId);
      case "set_model":
        this.options.onSetModel?.(request.request.model);
        return makeControlSuccess(request.request_id, this.sessionId);
      case "set_max_thinking_tokens":
        this.options.onSetMaxThinkingTokens?.(
          request.request.max_thinking_tokens ?? null,
        );
        return makeControlSuccess(request.request_id, this.sessionId);
      case "set_permission_mode": {
        const verdict = this.options.onSetPermissionMode?.(request.request.mode ?? "") ?? {
          ok: false as const,
          error: "Permission mode changes are not supported in this bridge.",
        };
        return verdict.ok
          ? makeControlSuccess(request.request_id, this.sessionId)
          : makeControlError(
              request.request_id,
              verdict.error,
              this.sessionId,
            );
      }
      case "interrupt":
        this.options.onInterrupt?.();
        return makeControlSuccess(request.request_id, this.sessionId);
      default:
        return makeControlError(
          request.request_id,
          `Unsupported control request subtype: ${subtype}`,
          this.sessionId,
        );
    }
  }

  private async handlePermissionRequest(
    request: BridgeControlRequest,
  ): Promise<void> {
    if (!this.permissionBroker) {
      await this.sendControlResponse(
        makeControlError(
          request.request_id,
          "Permission broker is not configured for this bridge.",
          this.sessionId,
        ),
      );
      return;
    }

    const toolName = request.request.tool_name ?? "unknown";
    const input = request.request.input ?? {};
    const toolUseId = request.request.tool_use_id ?? request.request_id;
    const responsePromise = this.permissionBroker.waitForResponse(
      request.request_id,
      this.permissionTimeoutMs,
    );
    this.permissionBroker.sendRequest({
      requestId: request.request_id,
      toolName,
      input,
      toolUseId,
      description:
        typeof request.request.description === "string"
          ? request.request.description
          : undefined,
      permissionSuggestions: Array.isArray(request.request.permission_suggestions)
        ? request.request.permission_suggestions
        : undefined,
      blockedPath:
        typeof request.request.blocked_path === "string"
          ? request.request.blocked_path
          : undefined,
    });

    try {
      const response = await responsePromise;
      await this.sendControlResponse(
        makeControlSuccess(
          request.request_id,
          this.sessionId,
          response as unknown as Record<string, unknown>,
        ),
      );
    } catch (error) {
      await this.sendControlResponse(
        makeControlError(
          request.request_id,
          formatError(error),
          this.sessionId,
        ),
      );
    }
  }

  private prepareOutboundMessage(message: BridgeChatMessage): BridgeChatMessage {
    return this.prepareWireMessage(message) as BridgeChatMessage;
  }

  private prepareWireMessage(message: BridgeWireMessage): BridgeWireMessage {
    const withSession = {
      ...message,
      ...(message.session_id ? {} : { session_id: this.sessionId }),
    } as BridgeWireMessage;
    if ("uuid" in withSession) {
      const uuid = withSession.uuid ?? randomUUID();
      withSession.uuid = uuid;
      this.recentPostedUUIDs.add(uuid);
    }
    return withSession;
  }

  private setState(state: BridgeState, detail?: string): void {
    this.state = state;
    this.options.onStateChange?.(state, detail);
  }
}

export async function createReplBridge(
  options: ReplBridgeOptions,
): Promise<ReplBridgeHandle> {
  const controller = new ReplBridgeController(options);
  await controller.start();
  return controller;
}

function formatError(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}
