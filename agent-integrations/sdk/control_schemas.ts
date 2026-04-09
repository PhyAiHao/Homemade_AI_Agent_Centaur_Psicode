import { z } from "zod";

import {
  AccountInfoSchema,
  AgentDefinitionSchema,
  AgentInfoSchema,
  FastModeStateSchema,
  HookEventSchema,
  HookInputSchema,
  McpServerConfigForProcessTransportSchema,
  McpServerStatusSchema,
  ModelInfoSchema,
  PermissionModeSchema,
  PermissionUpdateSchema,
  SDKMessageSchema,
  SDKPostTurnSummaryMessageSchema,
  SDKStreamlinedTextMessageSchema,
  SDKStreamlinedToolUseSummaryMessageSchema,
  SDKUserMessageSchema,
} from "./core_schemas.js";

export const JSONRPCMessagePlaceholderSchema = z.unknown();

export const SDKHookCallbackMatcherSchema = z.object({
  matcher: z.string().optional(),
  hookCallbackIds: z.array(z.string()),
  timeout: z.number().optional(),
});

export const SDKControlInitializeRequestSchema = z.object({
  subtype: z.literal("initialize"),
  hooks: z.record(HookEventSchema, z.array(SDKHookCallbackMatcherSchema)).optional(),
  sdkMcpServers: z.array(z.string()).optional(),
  jsonSchema: z.record(z.string(), z.unknown()).optional(),
  systemPrompt: z.string().optional(),
  appendSystemPrompt: z.string().optional(),
  agents: z.record(z.string(), AgentDefinitionSchema).optional(),
  promptSuggestions: z.boolean().optional(),
  agentProgressSummaries: z.boolean().optional(),
});

export const SDKControlInitializeResponseSchema = z.object({
  commands: z.array(AgentCommandSchema()),
  agents: z.array(AgentInfoSchema),
  output_style: z.string(),
  available_output_styles: z.array(z.string()),
  models: z.array(ModelInfoSchema),
  account: AccountInfoSchema,
  pid: z.number().optional(),
  fast_mode_state: FastModeStateSchema.optional(),
});

function AgentCommandSchema() {
  return z.object({
    name: z.string(),
    description: z.string().optional(),
    aliases: z.array(z.string()).optional(),
    argNames: z.array(z.string()).optional(),
  });
}

export const SDKControlInterruptRequestSchema = z.object({
  subtype: z.literal("interrupt"),
});

export const SDKControlPermissionRequestSchema = z.object({
  subtype: z.literal("can_use_tool"),
  tool_name: z.string(),
  input: z.record(z.string(), z.unknown()),
  permission_suggestions: z.array(PermissionUpdateSchema).optional(),
  blocked_path: z.string().optional(),
  decision_reason: z.string().optional(),
  title: z.string().optional(),
  display_name: z.string().optional(),
  tool_use_id: z.string(),
  agent_id: z.string().optional(),
  description: z.string().optional(),
});

export const SDKControlSetPermissionModeRequestSchema = z.object({
  subtype: z.literal("set_permission_mode"),
  mode: PermissionModeSchema,
  ultraplan: z.boolean().optional(),
});

export const SDKControlSetModelRequestSchema = z.object({
  subtype: z.literal("set_model"),
  model: z.string().optional(),
});

export const SDKControlSetMaxThinkingTokensRequestSchema = z.object({
  subtype: z.literal("set_max_thinking_tokens"),
  max_thinking_tokens: z.number().nullable(),
});

export const SDKControlMcpStatusRequestSchema = z.object({
  subtype: z.literal("mcp_status"),
});

export const SDKControlMcpStatusResponseSchema = z.object({
  mcpServers: z.array(McpServerStatusSchema),
});

export const SDKControlGetContextUsageRequestSchema = z.object({
  subtype: z.literal("get_context_usage"),
});

const ContextCategorySchema = z.object({
  name: z.string(),
  tokens: z.number(),
  color: z.string(),
  isDeferred: z.boolean().optional(),
});

export const SDKControlGetContextUsageResponseSchema = z.object({
  categories: z.array(ContextCategorySchema),
  totalTokens: z.number(),
  maxTokens: z.number(),
  rawMaxTokens: z.number(),
  percentage: z.number(),
  model: z.string(),
  isAutoCompactEnabled: z.boolean(),
  autoCompactThreshold: z.number().optional(),
});

export const SDKControlRewindFilesRequestSchema = z.object({
  subtype: z.literal("rewind_files"),
  user_message_id: z.string(),
  dry_run: z.boolean().optional(),
});

export const SDKControlRewindFilesResponseSchema = z.object({
  canRewind: z.boolean(),
  error: z.string().optional(),
  filesChanged: z.array(z.string()).optional(),
  insertions: z.number().optional(),
  deletions: z.number().optional(),
});

export const SDKControlCancelAsyncMessageRequestSchema = z.object({
  subtype: z.literal("cancel_async_message"),
  message_uuid: z.string(),
});

export const SDKControlCancelAsyncMessageResponseSchema = z.object({
  cancelled: z.boolean(),
});

export const SDKHookCallbackRequestSchema = z.object({
  subtype: z.literal("hook_callback"),
  callback_id: z.string(),
  input: HookInputSchema,
  tool_use_id: z.string().optional(),
});

export const SDKControlMcpMessageRequestSchema = z.object({
  subtype: z.literal("mcp_message"),
  server_name: z.string(),
  message: JSONRPCMessagePlaceholderSchema,
});

export const SDKControlMcpSetServersRequestSchema = z.object({
  subtype: z.literal("mcp_set_servers"),
  servers: z.record(z.string(), McpServerConfigForProcessTransportSchema),
});

export const SDKControlMcpSetServersResponseSchema = z.object({
  added: z.array(z.string()),
  removed: z.array(z.string()),
  errors: z.record(z.string(), z.string()),
});

export const SDKControlReloadPluginsRequestSchema = z.object({
  subtype: z.literal("reload_plugins"),
});

export const SDKControlReloadPluginsResponseSchema = z.object({
  commands: z.array(AgentCommandSchema()),
  agents: z.array(AgentInfoSchema),
  plugins: z.array(
    z.object({
      name: z.string(),
      path: z.string(),
      source: z.string().optional(),
    }),
  ),
  mcpServers: z.array(McpServerStatusSchema),
  error_count: z.number(),
});

export const SDKControlMcpReconnectRequestSchema = z.object({
  subtype: z.literal("mcp_reconnect"),
  serverName: z.string(),
});

export const SDKControlMcpToggleRequestSchema = z.object({
  subtype: z.literal("mcp_toggle"),
  serverName: z.string(),
  enabled: z.boolean(),
});

export const SDKControlStopTaskRequestSchema = z.object({
  subtype: z.literal("stop_task"),
  task_id: z.string(),
});

export const SDKControlApplyFlagSettingsRequestSchema = z.object({
  subtype: z.literal("apply_flag_settings"),
  settings: z.record(z.string(), z.unknown()),
});

export const SDKControlGetSettingsRequestSchema = z.object({
  subtype: z.literal("get_settings"),
});

export const SDKControlGetSettingsResponseSchema = z.object({
  effective: z.record(z.string(), z.unknown()),
  sources: z.array(
    z.object({
      source: z.enum([
        "userSettings",
        "projectSettings",
        "localSettings",
        "flagSettings",
        "policySettings",
      ]),
      settings: z.record(z.string(), z.unknown()),
    }),
  ),
  applied: z
    .object({
      model: z.string(),
      effort: z.enum(["low", "medium", "high", "max"]).nullable(),
    })
    .optional(),
});

export const SDKControlRequestSchema = z.union([
  SDKControlInitializeRequestSchema,
  SDKControlInterruptRequestSchema,
  SDKControlPermissionRequestSchema,
  SDKControlSetPermissionModeRequestSchema,
  SDKControlSetModelRequestSchema,
  SDKControlSetMaxThinkingTokensRequestSchema,
  SDKControlMcpStatusRequestSchema,
  SDKControlGetContextUsageRequestSchema,
  SDKControlRewindFilesRequestSchema,
  SDKControlCancelAsyncMessageRequestSchema,
  SDKHookCallbackRequestSchema,
  SDKControlMcpMessageRequestSchema,
  SDKControlMcpSetServersRequestSchema,
  SDKControlReloadPluginsRequestSchema,
  SDKControlMcpReconnectRequestSchema,
  SDKControlMcpToggleRequestSchema,
  SDKControlStopTaskRequestSchema,
  SDKControlApplyFlagSettingsRequestSchema,
  SDKControlGetSettingsRequestSchema,
]);

export const SDKControlResponseSchema = z.union([
  SDKControlInitializeResponseSchema,
  SDKControlMcpStatusResponseSchema,
  SDKControlGetContextUsageResponseSchema,
  SDKControlRewindFilesResponseSchema,
  SDKControlCancelAsyncMessageResponseSchema,
  SDKControlMcpSetServersResponseSchema,
  SDKControlReloadPluginsResponseSchema,
  SDKControlGetSettingsResponseSchema,
  SDKMessageSchema,
  SDKUserMessageSchema,
  SDKStreamlinedTextMessageSchema,
  SDKStreamlinedToolUseSummaryMessageSchema,
  SDKPostTurnSummaryMessageSchema,
]);
