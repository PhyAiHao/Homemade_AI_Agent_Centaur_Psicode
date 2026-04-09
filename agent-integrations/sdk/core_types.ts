export type JsonPrimitive = string | number | boolean | null;

export type JsonValue =
  | JsonPrimitive
  | JsonValue[]
  | {
      [key: string]: JsonValue;
    };

export const HOOK_EVENTS = [
  "PreToolUse",
  "PostToolUse",
  "PostToolUseFailure",
  "Notification",
  "UserPromptSubmit",
  "SessionStart",
  "SessionEnd",
  "Stop",
  "StopFailure",
  "SubagentStart",
  "SubagentStop",
  "PreCompact",
  "PostCompact",
  "PermissionRequest",
  "PermissionDenied",
  "Setup",
  "TeammateIdle",
  "TaskCreated",
  "TaskCompleted",
  "Elicitation",
  "ElicitationResult",
  "ConfigChange",
  "WorktreeCreate",
  "WorktreeRemove",
  "InstructionsLoaded",
  "CwdChanged",
  "FileChanged",
] as const;

export const EXIT_REASONS = [
  "clear",
  "resume",
  "logout",
  "prompt_input_exit",
  "other",
  "bypass_permissions_disabled",
] as const;

export type HookEvent = (typeof HOOK_EVENTS)[number];
export type ExitReason = (typeof EXIT_REASONS)[number];

export interface ModelUsage {
  inputTokens: number;
  outputTokens: number;
  cacheReadInputTokens: number;
  cacheCreationInputTokens: number;
  webSearchRequests: number;
  costUSD: number;
  contextWindow: number;
  maxOutputTokens: number;
}

export interface JsonSchemaOutputFormat {
  type: "json_schema";
  schema: Record<string, unknown>;
}

export type OutputFormat = JsonSchemaOutputFormat;

export type ApiKeySource = "user" | "project" | "org" | "temporary" | "oauth";
export type ConfigScope = "local" | "user" | "project";
export type SdkBeta = "context-1m-2025-08-07";

export type ThinkingConfig =
  | {
      type: "adaptive";
    }
  | {
      type: "enabled";
      budgetTokens?: number;
    }
  | {
      type: "disabled";
    };

export type PermissionMode =
  | "acceptEdits"
  | "bypassPermissions"
  | "default"
  | "dontAsk"
  | "plan";

export type PermissionUpdateDestination =
  | "userSettings"
  | "projectSettings"
  | "localSettings"
  | "session"
  | "cliArg";

export type PermissionBehavior = "allow" | "deny" | "ask";

export interface PermissionRuleValue {
  toolName: string;
  ruleContent?: string;
}

export type PermissionUpdate =
  | {
      type: "addRules" | "replaceRules" | "removeRules";
      destination: PermissionUpdateDestination;
      rules: PermissionRuleValue[];
      behavior: PermissionBehavior;
    }
  | {
      type: "setMode";
      destination: PermissionUpdateDestination;
      mode: PermissionMode;
    }
  | {
      type: "addDirectories" | "removeDirectories";
      destination: PermissionUpdateDestination;
      directories: string[];
    };

export type McpServerConfigForProcessTransport =
  | {
      type?: "stdio";
      command: string;
      args?: string[];
      env?: Record<string, string>;
    }
  | {
      type: "sse" | "http";
      url: string;
      headers?: Record<string, string>;
    }
  | {
      type: "sdk";
      name: string;
    };

export interface McpToolInfo {
  name: string;
  description?: string;
  annotations?: {
    readOnly?: boolean;
    destructive?: boolean;
    openWorld?: boolean;
  };
}

export interface McpServerStatus {
  name: string;
  status: "connected" | "failed" | "needs-auth" | "pending" | "disabled";
  error?: string;
  scope?: string;
  serverInfo?: {
    name: string;
    version: string;
  };
  config?: McpServerConfigForProcessTransport | { type: "claudeai-proxy"; url: string; id: string };
  tools?: McpToolInfo[];
}

export interface HookInput {
  event: HookEvent;
  payload: Record<string, JsonValue>;
  toolUseId?: string;
  cwd?: string;
  sessionId?: string;
}

export interface SlashCommand {
  name: string;
  description?: string;
  aliases?: string[];
  argNames?: string[];
}

export interface AgentDefinition {
  prompt: string;
  description?: string;
  tools?: string[];
  model?: string;
  permissionMode?: PermissionMode;
}

export interface AgentInfo {
  name: string;
  description?: string;
  source?: string;
  prompt?: string;
  tools?: string[];
  model?: string;
}

export interface ModelInfo {
  id: string;
  display_name?: string;
  provider?: string;
  context_window?: number;
  max_output_tokens?: number;
  supports_thinking?: boolean;
}

export interface AccountInfo {
  email?: string;
  organization?: string;
  plan?: string;
  source?: ApiKeySource;
  status?: string;
}

export type FastModeState = "off" | "cooldown" | "on";

export interface SDKAssistantMessage {
  type: "assistant";
  uuid?: string;
  message: {
    role: "assistant";
    content: JsonValue;
    model?: string;
    usage?: ModelUsage;
  };
  timestamp?: number;
}

export interface SDKUserMessage {
  type: "user";
  uuid?: string;
  message: {
    role: "user";
    content: JsonValue;
  };
  timestamp?: number;
}

export interface SDKStreamlinedTextMessage {
  type: "streamlined_text";
  text: string;
  is_error?: boolean;
}

export interface SDKStreamlinedToolUseSummaryMessage {
  type: "streamlined_tool_use_summary";
  tool_name: string;
  summary: string;
  is_error?: boolean;
}

export interface SDKPostTurnSummaryMessage {
  type: "post_turn_summary";
  summary: string;
  request_id?: string;
}

export type SDKMessage =
  | SDKAssistantMessage
  | SDKUserMessage
  | SDKStreamlinedTextMessage
  | SDKStreamlinedToolUseSummaryMessage
  | SDKPostTurnSummaryMessage;
