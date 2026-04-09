import { randomUUID } from "node:crypto";

import type {
  BridgeChatMessage,
  BridgeControlRequest,
  BridgeControlResponse,
  BridgeResultMessage,
  BridgeTextBlock,
  BridgeWireMessage,
} from "./types.js";

const KEY_RENAMES: Record<string, string> = {
  requestId: "request_id",
  toolName: "tool_name",
  toolUseId: "tool_use_id",
  sessionId: "session_id",
  maxThinkingTokens: "max_thinking_tokens",
  blockedPath: "blocked_path",
  permissionSuggestions: "permission_suggestions",
  parentToolUseId: "parent_tool_use_id",
  isSynthetic: "isSynthetic",
  isReplay: "isReplay",
};

export function parseBridgePayload(
  payload: string | Record<string, unknown>,
): BridgeWireMessage | Record<string, unknown> | null {
  const parsed = typeof payload === "string" ? safeJsonParse(payload) : payload;
  if (!parsed || typeof parsed !== "object") {
    return null;
  }
  const normalized = normalizeControlMessageKeys(parsed);
  return normalized;
}

export function normalizeControlMessageKeys<T>(value: T): T {
  if (Array.isArray(value)) {
    return value.map(item => normalizeControlMessageKeys(item)) as T;
  }
  if (!value || typeof value !== "object") {
    return value;
  }
  const result: Record<string, unknown> = {};
  for (const [key, entry] of Object.entries(value as Record<string, unknown>)) {
    const nextKey = KEY_RENAMES[key] ?? key;
    result[nextKey] = normalizeControlMessageKeys(entry);
  }
  return result as T;
}

export function isBridgeChatMessage(value: unknown): value is BridgeChatMessage {
  return (
    !!value &&
    typeof value === "object" &&
    "type" in value &&
    typeof (value as { type: unknown }).type === "string" &&
    ["user", "assistant", "system"].includes((value as { type: string }).type) &&
    "message" in value
  );
}

export function isBridgeControlRequest(
  value: unknown,
): value is BridgeControlRequest {
  return (
    !!value &&
    typeof value === "object" &&
    "type" in value &&
    (value as { type?: unknown }).type === "control_request" &&
    "request_id" in value &&
    "request" in value
  );
}

export function isBridgeControlResponse(
  value: unknown,
): value is BridgeControlResponse {
  return (
    !!value &&
    typeof value === "object" &&
    "type" in value &&
    (value as { type?: unknown }).type === "control_response" &&
    "response" in value
  );
}

export function isBridgeResultMessage(
  value: unknown,
): value is BridgeResultMessage {
  return (
    !!value &&
    typeof value === "object" &&
    "type" in value &&
    (value as { type?: unknown }).type === "result" &&
    "subtype" in value
  );
}

export function isBridgeWireMessage(value: unknown): value is BridgeWireMessage {
  return (
    isBridgeChatMessage(value) ||
    isBridgeControlRequest(value) ||
    isBridgeControlResponse(value) ||
    isBridgeResultMessage(value)
  );
}

export function isEligibleBridgeMessage(message: BridgeChatMessage): boolean {
  if (
    (message.type === "user" || message.type === "assistant") &&
    message.isVirtual
  ) {
    return false;
  }
  return (
    message.type === "user" ||
    message.type === "assistant" ||
    (message.type === "system" && message.subtype === "local_command")
  );
}

export function extractTitleText(message: BridgeChatMessage): string | undefined {
  if (
    message.type !== "user" ||
    message.isMeta ||
    message.isCompactSummary ||
    message.parent_tool_use_id != null
  ) {
    return undefined;
  }
  if (message.origin?.kind && message.origin.kind !== "human") {
    return undefined;
  }
  const content = message.message.content;
  const raw = typeof content === "string" ? content : firstText(content);
  if (!raw) {
    return undefined;
  }
  const stripped = stripDisplayTags(raw);
  return stripped || undefined;
}

export function makeControlSuccess(
  requestId: string,
  sessionId?: string,
  response?: Record<string, unknown>,
): BridgeControlResponse {
  return {
    type: "control_response",
    ...(sessionId ? { session_id: sessionId } : {}),
    response: {
      subtype: "success",
      request_id: requestId,
      ...(response ? { response } : {}),
    },
  };
}

export function makeControlError(
  requestId: string,
  error: string,
  sessionId?: string,
): BridgeControlResponse {
  return {
    type: "control_response",
    ...(sessionId ? { session_id: sessionId } : {}),
    response: {
      subtype: "error",
      request_id: requestId,
      error,
    },
  };
}

export function makeInitializeResponse(
  requestId: string,
  sessionId?: string,
): BridgeControlResponse {
  return makeControlSuccess(requestId, sessionId, {
    commands: [],
    output_style: "normal",
    available_output_styles: ["normal"],
    models: [],
    account: {},
    pid: typeof process !== "undefined" ? process.pid : 0,
  });
}

export function makeResultMessage(
  sessionId: string,
  result = "",
  subtype: "success" | "error" = "success",
  errors: string[] = [],
): BridgeResultMessage {
  const isError = subtype === "error";
  return {
    type: "result",
    session_id: sessionId,
    uuid: randomUUID(),
    subtype,
    duration_ms: 0,
    duration_api_ms: 0,
    is_error: isError,
    num_turns: 0,
    result,
    stop_reason: null,
    total_cost_usd: 0,
    usage: {},
    modelUsage: {},
    permission_denials: [],
    ...(errors.length > 0 ? { errors } : {}),
  };
}

export class BoundedUUIDSet {
  private readonly ring: Array<string | undefined>;
  private readonly set = new Set<string>();
  private writeIndex = 0;

  constructor(private readonly capacity: number) {
    this.ring = new Array<string | undefined>(capacity);
  }

  add(uuid: string): void {
    if (this.set.has(uuid)) {
      return;
    }
    const existing = this.ring[this.writeIndex];
    if (existing !== undefined) {
      this.set.delete(existing);
    }
    this.ring[this.writeIndex] = uuid;
    this.set.add(uuid);
    this.writeIndex = (this.writeIndex + 1) % this.capacity;
  }

  has(uuid: string): boolean {
    return this.set.has(uuid);
  }

  clear(): void {
    this.set.clear();
    this.ring.fill(undefined);
    this.writeIndex = 0;
  }
}

function firstText(blocks: BridgeTextBlock[]): string {
  for (const block of blocks) {
    if (block.type === "text" && block.text.trim().length > 0) {
      return block.text;
    }
  }
  return "";
}

function stripDisplayTags(text: string): string {
  return text.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ").trim();
}

function safeJsonParse(value: string): Record<string, unknown> | null {
  try {
    return JSON.parse(value) as Record<string, unknown>;
  } catch {
    return null;
  }
}
