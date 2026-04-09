export type BridgeState =
  | "idle"
  | "connecting"
  | "ready"
  | "connected"
  | "reconnecting"
  | "failed"
  | "closed";

export type SessionDoneStatus = "completed" | "failed" | "interrupted";

export type SessionActivityType =
  | "inbound"
  | "outbound"
  | "control"
  | "result"
  | "error";

export interface SessionActivity {
  type: SessionActivityType;
  summary: string;
  timestamp: number;
}

export interface BridgeTextBlock {
  type: "text";
  text: string;
}

export interface BridgeStructuredPayload {
  role?: string;
  content: string | BridgeTextBlock[];
}

export interface BridgeChatMessage {
  type: "user" | "assistant" | "system";
  session_id?: string;
  uuid?: string;
  subtype?: string;
  message: BridgeStructuredPayload;
  isMeta?: boolean;
  isSynthetic?: boolean;
  isReplay?: boolean;
  isVirtual?: boolean;
  isCompactSummary?: boolean;
  parent_tool_use_id?: string;
  origin?: {
    kind?: string;
    [key: string]: unknown;
  };
}

export type BridgeControlSubtype =
  | "initialize"
  | "set_model"
  | "set_permission_mode"
  | "set_max_thinking_tokens"
  | "interrupt"
  | "can_use_tool"
  | (string & {});

export interface BridgeControlRequestPayload {
  subtype: BridgeControlSubtype;
  model?: string;
  mode?: string;
  max_thinking_tokens?: number | null;
  tool_name?: string;
  input?: Record<string, unknown>;
  tool_use_id?: string;
  description?: string;
  permission_suggestions?: unknown[];
  blocked_path?: string;
  [key: string]: unknown;
}

export interface BridgeControlRequest {
  type: "control_request";
  session_id?: string;
  request_id: string;
  request: BridgeControlRequestPayload;
}

export interface BridgeControlResponsePayload {
  subtype: "success" | "error";
  request_id: string;
  response?: Record<string, unknown>;
  error?: string;
}

export interface BridgeControlResponse {
  type: "control_response";
  session_id?: string;
  response: BridgeControlResponsePayload;
}

export interface BridgeResultMessage {
  type: "result";
  session_id?: string;
  uuid?: string;
  subtype: "success" | "error";
  duration_ms: number;
  duration_api_ms: number;
  is_error: boolean;
  num_turns: number;
  result: string;
  stop_reason: string | null;
  total_cost_usd: number;
  usage: Record<string, unknown>;
  modelUsage: Record<string, unknown>;
  permission_denials: unknown[];
  errors?: string[];
}

export type BridgeWireMessage =
  | BridgeChatMessage
  | BridgeControlRequest
  | BridgeControlResponse
  | BridgeResultMessage;

export interface BridgeTransport
  extends AsyncIterable<string | Record<string, unknown>>
{
  connect(): Promise<void>;
  send(message: BridgeWireMessage | Record<string, unknown>): Promise<void>;
  sendBatch(
    messages: Array<BridgeWireMessage | Record<string, unknown>>,
  ): Promise<void>;
  close(code?: number, reason?: string): Promise<void>;
  getStateLabel(): string;
  isConnected(): boolean;
  updateAuthToken?(token: string): void;
}

export interface SessionRunnerSpawnOptions {
  sessionId: string;
  accessToken?: string;
  onActivity?: (sessionId: string, activity: SessionActivity) => void;
  onInboundMessage?: (message: BridgeChatMessage) => void | Promise<void>;
  onControlResponse?: (response: BridgeControlResponse) => void;
}

export interface BridgeSessionHandle {
  sessionId: string;
  accessToken?: string;
  done: Promise<SessionDoneStatus>;
  activities: SessionActivity[];
  currentActivity: SessionActivity | null;
  inboundMessages: BridgeChatMessage[];
  outboundMessages: BridgeChatMessage[];
  controlResponses: BridgeControlResponse[];
  writeInbound(message: BridgeChatMessage): Promise<void>;
  writeOutbound(message: BridgeChatMessage): void;
  writeControlResponse(response: BridgeControlResponse): void;
  complete(status: SessionDoneStatus, result?: string): void;
  fail(error: string): void;
  interrupt(): void;
  updateAccessToken(token: string): void;
}

export interface SessionRunner {
  spawn(options: SessionRunnerSpawnOptions): BridgeSessionHandle;
}

export interface BridgePermissionResponse {
  behavior: "allow" | "deny";
  updatedInput?: Record<string, unknown>;
  updatedPermissions?: unknown[];
  message?: string;
}

export interface BridgePermissionRequest {
  requestId: string;
  toolName: string;
  input: Record<string, unknown>;
  toolUseId: string;
  description?: string;
  permissionSuggestions?: unknown[];
  blockedPath?: string;
}

export interface BridgePermissionCallbacks {
  sendRequest(request: BridgePermissionRequest): void;
  sendResponse(requestId: string, response: BridgePermissionResponse): void;
  cancelRequest(requestId: string): void;
  onResponse(
    requestId: string,
    handler: (response: BridgePermissionResponse) => void,
  ): () => void;
}

export interface BridgePermissionBroker extends BridgePermissionCallbacks {
  onRequest(handler: (request: BridgePermissionRequest) => void): () => void;
  waitForResponse(
    requestId: string,
    timeoutMs?: number,
  ): Promise<BridgePermissionResponse>;
  getPendingRequests(): BridgePermissionRequest[];
}

export interface ReplBridgeHooks {
  onInboundMessage?: (message: BridgeChatMessage) => void | Promise<void>;
  onPermissionResponse?: (response: BridgeControlResponse) => void;
  onInterrupt?: () => void;
  onSetModel?: (model: string | undefined) => void;
  onSetMaxThinkingTokens?: (maxTokens: number | null) => void;
  onSetPermissionMode?: (
    mode: string,
  ) => { ok: true } | { ok: false; error: string };
  onStateChange?: (state: BridgeState, detail?: string) => void;
  onUserMessage?: (text: string, sessionId: string) => boolean | void;
}

export interface ReplBridgeOptions extends ReplBridgeHooks {
  sessionId: string;
  accessToken?: string;
  transport: BridgeTransport;
  sessionRunner?: SessionRunner;
  initialMessages?: BridgeChatMessage[];
  permissionBroker?: BridgePermissionBroker;
  outboundOnly?: boolean;
  permissionTimeoutMs?: number;
}

export interface ReplBridgeHandle {
  bridgeSessionId: string;
  sessionId: string;
  writeMessages(messages: BridgeChatMessage[]): Promise<void>;
  writeSdkMessages(messages: BridgeWireMessage[]): Promise<void>;
  sendControlRequest(request: BridgeControlRequest): Promise<void>;
  sendControlResponse(response: BridgeControlResponse): Promise<void>;
  sendControlCancelRequest(requestId: string): void;
  sendResult(result?: string): Promise<void>;
  replaceTransport(transport: BridgeTransport): Promise<void>;
  teardown(): Promise<void>;
  getState(): BridgeState;
  getSessionHandle(): BridgeSessionHandle;
}

export interface RemoteBridgeSession {
  sessionId: string;
  title: string;
}

export interface RemoteBridgeCredentials {
  bridgeToken: string;
  expiresInSeconds?: number;
  websocketUrl?: string;
  sseUrl?: string;
  postUrl?: string;
  headers?: Record<string, string>;
  metadata?: Record<string, unknown>;
}

export interface RemoteBridgeClient {
  createSession(input: {
    title: string;
    accessToken: string;
    metadata?: Record<string, unknown>;
  }): Promise<RemoteBridgeSession>;
  openBridge(input: {
    sessionId: string;
    accessToken: string;
  }): Promise<RemoteBridgeCredentials>;
  archiveSession?(input: {
    sessionId: string;
    accessToken: string;
  }): Promise<void>;
}

export type BridgeTransportFactory = (input: {
  sessionId: string;
  credentials: RemoteBridgeCredentials;
}) => Promise<BridgeTransport> | BridgeTransport;

export interface RemoteBridgeConnectOptions extends ReplBridgeHooks {
  title: string;
  accessToken?: string;
  metadata?: Record<string, unknown>;
  initialMessages?: BridgeChatMessage[];
  permissionBroker?: BridgePermissionBroker;
  outboundOnly?: boolean;
  permissionTimeoutMs?: number;
}

export interface ConnectedRemoteBridge {
  sessionId: string;
  title: string;
  bridge: ReplBridgeHandle;
  close(): Promise<void>;
}

export interface BridgeMainLoopConfig {
  maxSessions?: number;
  logger?: {
    info(message: string): void;
    warn(message: string): void;
    error(message: string): void;
  };
}

export interface BridgeStatusSnapshot {
  activeSessions: number;
  maxSessions: number;
  sessions: Array<{
    sessionId: string;
    title: string;
    state: BridgeState;
  }>;
}
