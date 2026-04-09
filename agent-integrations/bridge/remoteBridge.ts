import { createTokenRefreshScheduler } from "./jwtUtils.js";
import { createReplBridge } from "./replBridge.js";
import type {
  BridgeTransport,
  BridgeTransportFactory,
  ConnectedRemoteBridge,
  RemoteBridgeClient,
  RemoteBridgeConnectOptions,
  RemoteBridgeCredentials,
} from "./types.js";
import { HybridTransport } from "../transports/hybrid.js";

type FetchLike = typeof fetch;

export class HybridBridgeTransport implements BridgeTransport {
  private session:
    | import("../transports/hybrid.js").HybridTransportSession
    | undefined;
  private connected = false;
  private authToken: string | undefined;

  constructor(
    private readonly options: {
      websocketUrl?: string;
      sseUrl?: string;
      postUrl?: string;
      headers?: Record<string, string>;
      token?: string;
      hybrid?: HybridTransport;
      fetchImpl?: FetchLike;
    },
  ) {
    this.authToken = options.token;
  }

  async connect(): Promise<void> {
    const hybrid = this.options.hybrid ?? new HybridTransport();
    this.session = await hybrid.connect({
      websocketUrl: this.options.websocketUrl,
      sseUrl: this.options.sseUrl,
      requestInit: {
        headers: this.requestHeaders(),
      },
    });
    this.connected = true;
  }

  async send(message: Record<string, unknown>): Promise<void> {
    await this.ensureConnected();
    if (this.session?.mode === "websocket") {
      this.session.sendJson(message);
      return;
    }
    await this.postMessages([message]);
  }

  async sendBatch(messages: Array<Record<string, unknown>>): Promise<void> {
    await this.ensureConnected();
    if (this.session?.mode === "websocket") {
      for (const message of messages) {
        this.session.sendJson(message);
      }
      return;
    }
    await this.postMessages(messages);
  }

  async close(code?: number, reason?: string): Promise<void> {
    this.connected = false;
    this.session?.close(code, reason);
    this.session = undefined;
  }

  getStateLabel(): string {
    if (!this.session) {
      return "idle";
    }
    return this.session.mode;
  }

  isConnected(): boolean {
    return this.connected;
  }

  updateAuthToken(token: string): void {
    this.authToken = token;
  }

  async *[Symbol.asyncIterator](): AsyncIterator<string | Record<string, unknown>> {
    if (!this.session) {
      throw new Error("Hybrid bridge transport is not connected.");
    }
    for await (const event of this.session) {
      if (event.transport === "sse") {
        yield event.data;
        continue;
      }
      const payload = event.data;
      if (typeof payload === "string") {
        yield payload;
      } else if (payload instanceof ArrayBuffer) {
        yield new TextDecoder().decode(new Uint8Array(payload));
      } else if (ArrayBuffer.isView(payload)) {
        yield new TextDecoder().decode(payload);
      } else if (payload instanceof Blob) {
        const text = await payload.text();
        yield text;
      }
    }
  }

  private async ensureConnected(): Promise<void> {
    if (!this.connected || !this.session) {
      await this.connect();
    }
  }

  private async postMessages(messages: Array<Record<string, unknown>>): Promise<void> {
    if (!this.options.postUrl) {
      throw new Error(
        "SSE bridge transport requires postUrl for outbound messages.",
      );
    }
    const response = await this.fetchImpl()(this.options.postUrl, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        ...this.requestHeaders(),
      },
      body: JSON.stringify(messages.length === 1 ? messages[0] : messages),
    });
    if (!response.ok) {
      throw new Error(
        `Bridge POST failed with ${response.status} ${response.statusText}`,
      );
    }
  }

  private requestHeaders(): Record<string, string> {
    return {
      ...(this.options.headers ?? {}),
      ...(this.authToken
        ? { Authorization: `Bearer ${this.authToken}` }
        : {}),
    };
  }

  private fetchImpl(): FetchLike {
    if (this.options.fetchImpl) {
      return this.options.fetchImpl;
    }
    if (!globalThis.fetch) {
      throw new Error("global fetch is not available in this runtime");
    }
    return globalThis.fetch.bind(globalThis);
  }
}

export function createHybridBridgeTransportFactory(options: {
  hybrid?: HybridTransport;
  fetchImpl?: FetchLike;
} = {}): BridgeTransportFactory {
  return ({ credentials }) =>
    new HybridBridgeTransport({
      websocketUrl: credentials.websocketUrl,
      sseUrl: credentials.sseUrl,
      postUrl: credentials.postUrl,
      headers: credentials.headers,
      token: credentials.bridgeToken,
      hybrid: options.hybrid,
      fetchImpl: options.fetchImpl,
    });
}

export class FetchRemoteBridgeClient implements RemoteBridgeClient {
  constructor(
    private readonly baseUrl: string,
    private readonly fetchImpl: FetchLike = globalThis.fetch.bind(globalThis),
  ) {}

  async createSession(input: {
    title: string;
    accessToken: string;
    metadata?: Record<string, unknown>;
  }): Promise<{ sessionId: string; title: string }> {
    const response = await this.fetchImpl(`${this.baseUrl}/v1/code/sessions`, {
      method: "POST",
      headers: this.authHeaders(input.accessToken),
      body: JSON.stringify({
        title: input.title,
        ...(input.metadata ? { metadata: input.metadata } : {}),
      }),
    });
    const payload = (await response.json()) as Record<string, unknown>;
    if (!response.ok) {
      throw new Error(
        `Session creation failed with ${response.status} ${response.statusText}`,
      );
    }
    const sessionId = readString(payload, ["id", "session_id", "sessionId"]);
    if (!sessionId) {
      throw new Error("Session creation response did not include a session id.");
    }
    return {
      sessionId,
      title: readString(payload, ["title"]) ?? input.title,
    };
  }

  async openBridge(input: {
    sessionId: string;
    accessToken: string;
  }): Promise<RemoteBridgeCredentials> {
    const response = await this.fetchImpl(
      `${this.baseUrl}/v1/code/sessions/${encodeURIComponent(input.sessionId)}/bridge`,
      {
        method: "POST",
        headers: this.authHeaders(input.accessToken),
      },
    );
    const payload = (await response.json()) as Record<string, unknown>;
    if (!response.ok) {
      throw new Error(
        `Bridge credential request failed with ${response.status} ${response.statusText}`,
      );
    }
    const bridgeToken = readString(payload, [
      "worker_jwt",
      "bridge_token",
      "bridgeToken",
      "token",
    ]);
    if (!bridgeToken) {
      throw new Error("Bridge credential response did not include a token.");
    }
    return {
      bridgeToken,
      expiresInSeconds: readNumber(payload, [
        "expires_in",
        "expiresIn",
      ]),
      websocketUrl: readString(payload, [
        "websocket_url",
        "websocketUrl",
      ]),
      sseUrl: readString(payload, ["sse_url", "sseUrl"]),
      postUrl: readString(payload, ["post_url", "postUrl"]),
      headers:
        payload["headers"] && typeof payload["headers"] === "object"
          ? (payload["headers"] as Record<string, string>)
          : undefined,
      metadata:
        payload["metadata"] && typeof payload["metadata"] === "object"
          ? (payload["metadata"] as Record<string, unknown>)
          : undefined,
    };
  }

  async archiveSession(input: {
    sessionId: string;
    accessToken: string;
  }): Promise<void> {
    await this.fetchImpl(
      `${this.baseUrl}/v1/code/sessions/${encodeURIComponent(input.sessionId)}/archive`,
      {
        method: "POST",
        headers: this.authHeaders(input.accessToken),
      },
    );
  }

  private authHeaders(accessToken: string): Record<string, string> {
    return {
      Authorization: `Bearer ${accessToken}`,
      "content-type": "application/json",
    };
  }
}

export class RemoteBridge {
  constructor(
    private readonly options: {
      client: RemoteBridgeClient;
      transportFactory: BridgeTransportFactory;
      getAccessToken: () => string | undefined | Promise<string | undefined>;
    },
  ) {}

  async connect(
    input: RemoteBridgeConnectOptions,
  ): Promise<ConnectedRemoteBridge> {
    const initialAccessToken = input.accessToken ?? (await this.options.getAccessToken());
    if (!initialAccessToken) {
      throw new Error("Remote bridge requires an access token.");
    }

    const session = await this.options.client.createSession({
      title: input.title,
      accessToken: initialAccessToken,
      metadata: input.metadata,
    });
    const credentials = await this.options.client.openBridge({
      sessionId: session.sessionId,
      accessToken: initialAccessToken,
    });
    const transport = await this.options.transportFactory({
      sessionId: session.sessionId,
      credentials,
    });
    const bridge = await createReplBridge({
      sessionId: session.sessionId,
      accessToken: credentials.bridgeToken,
      transport,
      initialMessages: input.initialMessages,
      permissionBroker: input.permissionBroker,
      outboundOnly: input.outboundOnly,
      permissionTimeoutMs: input.permissionTimeoutMs,
      onInboundMessage: input.onInboundMessage,
      onPermissionResponse: input.onPermissionResponse,
      onInterrupt: input.onInterrupt,
      onSetModel: input.onSetModel,
      onSetMaxThinkingTokens: input.onSetMaxThinkingTokens,
      onSetPermissionMode: input.onSetPermissionMode,
      onStateChange: input.onStateChange,
      onUserMessage: input.onUserMessage,
    });

    const scheduler = createTokenRefreshScheduler({
      getAccessToken: this.options.getAccessToken,
      label: "remote-bridge",
      onRefresh: async (sessionId, accessToken) => {
        const refreshedCredentials = await this.options.client.openBridge({
          sessionId,
          accessToken,
        });
        const nextTransport = await this.options.transportFactory({
          sessionId,
          credentials: refreshedCredentials,
        });
        bridge.getSessionHandle().updateAccessToken(refreshedCredentials.bridgeToken);
        await bridge.replaceTransport(nextTransport);
      },
    });

    if (credentials.expiresInSeconds) {
      scheduler.scheduleFromExpiresIn(
        session.sessionId,
        credentials.expiresInSeconds,
      );
    } else {
      scheduler.schedule(session.sessionId, credentials.bridgeToken);
    }

    return {
      sessionId: session.sessionId,
      title: session.title,
      bridge,
      close: async () => {
        scheduler.cancelAll();
        await bridge.teardown();
        const finalAccessToken =
          input.accessToken ?? (await this.options.getAccessToken());
        if (finalAccessToken && this.options.client.archiveSession) {
          await this.options.client.archiveSession({
            sessionId: session.sessionId,
            accessToken: finalAccessToken,
          });
        }
      },
    };
  }
}

export function createRemoteBridge(options: {
  baseUrl: string;
  getAccessToken: () => string | undefined | Promise<string | undefined>;
  fetchImpl?: FetchLike;
  hybrid?: HybridTransport;
}): RemoteBridge {
  const client = new FetchRemoteBridgeClient(
    options.baseUrl,
    options.fetchImpl ?? globalThis.fetch.bind(globalThis),
  );
  const transportFactory = createHybridBridgeTransportFactory({
    hybrid: options.hybrid,
    fetchImpl: options.fetchImpl,
  });
  return new RemoteBridge({
    client,
    transportFactory,
    getAccessToken: options.getAccessToken,
  });
}

function readString(
  payload: Record<string, unknown>,
  keys: string[],
): string | undefined {
  for (const key of keys) {
    const value = payload[key];
    if (typeof value === "string" && value.length > 0) {
      return value;
    }
  }
  return undefined;
}

function readNumber(
  payload: Record<string, unknown>,
  keys: string[],
): number | undefined {
  for (const key of keys) {
    const value = payload[key];
    if (typeof value === "number" && Number.isFinite(value)) {
      return value;
    }
  }
  return undefined;
}
