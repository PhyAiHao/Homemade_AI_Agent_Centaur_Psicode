export type JsonPrimitive = string | number | boolean | null;

export type JsonValue =
  | JsonPrimitive
  | JsonValue[]
  | {
      [key: string]: JsonValue;
    };

export type AgentBToolName =
  | "WebFetch"
  | "WebSearch"
  | "NotebookEdit"
  | "LSPTool"
  | "MCPTool"
  | "McpAuthTool"
  | "ListMcpResources"
  | "ReadMcpResource";

export interface ToolInvocationContext {
  requestId: string;
  toolCallId: string;
  signal?: AbortSignal;
}

export interface ToolResultMessage {
  type: "tool_result";
  request_id: string;
  tool_call_id: string;
  output: unknown;
  is_error?: boolean;
}

export interface ToolExecutionEnvelope<TOutput> {
  tool: AgentBToolName;
  ok: boolean;
  output: TOutput;
}

export interface ToolResultWriter {
  write(message: ToolResultMessage): Promise<void> | void;
}
