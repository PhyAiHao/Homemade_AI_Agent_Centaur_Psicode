import { z } from "zod";

import {
  EXIT_REASONS,
  HOOK_EVENTS,
  type JsonValue,
} from "./core_types.js";

export const JsonValueSchema: z.ZodType<JsonValue> = z.lazy(() =>
  z.union([
    z.string(),
    z.number(),
    z.boolean(),
    z.null(),
    z.array(JsonValueSchema),
    z.record(JsonValueSchema),
  ]),
);

export const ModelUsageSchema = z.object({
  inputTokens: z.number(),
  outputTokens: z.number(),
  cacheReadInputTokens: z.number(),
  cacheCreationInputTokens: z.number(),
  webSearchRequests: z.number(),
  costUSD: z.number(),
  contextWindow: z.number(),
  maxOutputTokens: z.number(),
});

export const OutputFormatTypeSchema = z.literal("json_schema");

export const JsonSchemaOutputFormatSchema = z.object({
  type: OutputFormatTypeSchema,
  schema: z.record(z.string(), z.unknown()),
});

export const OutputFormatSchema = JsonSchemaOutputFormatSchema;

export const ApiKeySourceSchema = z.enum([
  "user",
  "project",
  "org",
  "temporary",
  "oauth",
]);

export const ConfigScopeSchema = z.enum(["local", "user", "project"]);

export const SdkBetaSchema = z.literal("context-1m-2025-08-07");

export const ThinkingAdaptiveSchema = z.object({
  type: z.literal("adaptive"),
});

export const ThinkingEnabledSchema = z.object({
  type: z.literal("enabled"),
  budgetTokens: z.number().optional(),
});

export const ThinkingDisabledSchema = z.object({
  type: z.literal("disabled"),
});

export const ThinkingConfigSchema = z.union([
  ThinkingAdaptiveSchema,
  ThinkingEnabledSchema,
  ThinkingDisabledSchema,
]);

export const PermissionModeSchema = z.enum([
  "acceptEdits",
  "bypassPermissions",
  "default",
  "dontAsk",
  "plan",
]);

export const PermissionUpdateDestinationSchema = z.enum([
  "userSettings",
  "projectSettings",
  "localSettings",
  "session",
  "cliArg",
]);

export const PermissionBehaviorSchema = z.enum(["allow", "deny", "ask"]);

export const PermissionRuleValueSchema = z.object({
  toolName: z.string(),
  ruleContent: z.string().optional(),
});

export const PermissionUpdateSchema = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("addRules"),
    destination: PermissionUpdateDestinationSchema,
    rules: z.array(PermissionRuleValueSchema),
    behavior: PermissionBehaviorSchema,
  }),
  z.object({
    type: z.literal("replaceRules"),
    destination: PermissionUpdateDestinationSchema,
    rules: z.array(PermissionRuleValueSchema),
    behavior: PermissionBehaviorSchema,
  }),
  z.object({
    type: z.literal("removeRules"),
    destination: PermissionUpdateDestinationSchema,
    rules: z.array(PermissionRuleValueSchema),
    behavior: PermissionBehaviorSchema,
  }),
  z.object({
    type: z.literal("setMode"),
    destination: PermissionUpdateDestinationSchema,
    mode: PermissionModeSchema,
  }),
  z.object({
    type: z.literal("addDirectories"),
    destination: PermissionUpdateDestinationSchema,
    directories: z.array(z.string()),
  }),
  z.object({
    type: z.literal("removeDirectories"),
    destination: PermissionUpdateDestinationSchema,
    directories: z.array(z.string()),
  }),
]);

export const McpStdioServerConfigSchema = z.object({
  type: z.literal("stdio").optional(),
  command: z.string(),
  args: z.array(z.string()).optional(),
  env: z.record(z.string(), z.string()).optional(),
});

export const McpSSEServerConfigSchema = z.object({
  type: z.literal("sse"),
  url: z.string(),
  headers: z.record(z.string(), z.string()).optional(),
});

export const McpHttpServerConfigSchema = z.object({
  type: z.literal("http"),
  url: z.string(),
  headers: z.record(z.string(), z.string()).optional(),
});

export const McpSdkServerConfigSchema = z.object({
  type: z.literal("sdk"),
  name: z.string(),
});

export const McpServerConfigForProcessTransportSchema = z.union([
  McpStdioServerConfigSchema,
  McpSSEServerConfigSchema,
  McpHttpServerConfigSchema,
  McpSdkServerConfigSchema,
]);

export const McpServerStatusConfigSchema = z.union([
  McpServerConfigForProcessTransportSchema,
  z.object({
    type: z.literal("claudeai-proxy"),
    url: z.string(),
    id: z.string(),
  }),
]);

export const McpToolInfoSchema = z.object({
  name: z.string(),
  description: z.string().optional(),
  annotations: z
    .object({
      readOnly: z.boolean().optional(),
      destructive: z.boolean().optional(),
      openWorld: z.boolean().optional(),
    })
    .optional(),
});

export const McpServerStatusSchema = z.object({
  name: z.string(),
  status: z.enum(["connected", "failed", "needs-auth", "pending", "disabled"]),
  error: z.string().optional(),
  scope: z.string().optional(),
  serverInfo: z
    .object({
      name: z.string(),
      version: z.string(),
    })
    .optional(),
  config: McpServerStatusConfigSchema.optional(),
  tools: z.array(McpToolInfoSchema).optional(),
});

export const HookEventSchema = z.enum(HOOK_EVENTS);
export const ExitReasonSchema = z.enum(EXIT_REASONS);

export const HookInputSchema = z.object({
  event: HookEventSchema,
  payload: z.record(JsonValueSchema),
  toolUseId: z.string().optional(),
  cwd: z.string().optional(),
  sessionId: z.string().optional(),
});

export const SlashCommandSchema = z.object({
  name: z.string(),
  description: z.string().optional(),
  aliases: z.array(z.string()).optional(),
  argNames: z.array(z.string()).optional(),
});

export const AgentDefinitionSchema = z.object({
  prompt: z.string(),
  description: z.string().optional(),
  tools: z.array(z.string()).optional(),
  model: z.string().optional(),
  permissionMode: PermissionModeSchema.optional(),
});

export const AgentInfoSchema = z.object({
  name: z.string(),
  description: z.string().optional(),
  source: z.string().optional(),
  prompt: z.string().optional(),
  tools: z.array(z.string()).optional(),
  model: z.string().optional(),
});

export const ModelInfoSchema = z.object({
  id: z.string(),
  display_name: z.string().optional(),
  provider: z.string().optional(),
  context_window: z.number().optional(),
  max_output_tokens: z.number().optional(),
  supports_thinking: z.boolean().optional(),
});

export const AccountInfoSchema = z.object({
  email: z.string().optional(),
  organization: z.string().optional(),
  plan: z.string().optional(),
  source: ApiKeySourceSchema.optional(),
  status: z.string().optional(),
});

export const SDKAssistantMessageSchema = z.object({
  type: z.literal("assistant"),
  uuid: z.string().optional(),
  message: z.object({
    role: z.literal("assistant"),
    content: JsonValueSchema,
    model: z.string().optional(),
    usage: ModelUsageSchema.optional(),
  }),
  timestamp: z.number().optional(),
});

export const SDKUserMessageSchema = z.object({
  type: z.literal("user"),
  uuid: z.string().optional(),
  message: z.object({
    role: z.literal("user"),
    content: JsonValueSchema,
  }),
  timestamp: z.number().optional(),
});

export const SDKStreamlinedTextMessageSchema = z.object({
  type: z.literal("streamlined_text"),
  text: z.string(),
  is_error: z.boolean().optional(),
});

export const SDKStreamlinedToolUseSummaryMessageSchema = z.object({
  type: z.literal("streamlined_tool_use_summary"),
  tool_name: z.string(),
  summary: z.string(),
  is_error: z.boolean().optional(),
});

export const SDKPostTurnSummaryMessageSchema = z.object({
  type: z.literal("post_turn_summary"),
  summary: z.string(),
  request_id: z.string().optional(),
});

export const SDKMessageSchema = z.union([
  SDKAssistantMessageSchema,
  SDKUserMessageSchema,
  SDKStreamlinedTextMessageSchema,
  SDKStreamlinedToolUseSummaryMessageSchema,
  SDKPostTurnSummaryMessageSchema,
]);

export const FastModeStateSchema = z.enum(["off", "cooldown", "on"]);
