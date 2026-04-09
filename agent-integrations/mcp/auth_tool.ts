import type {
  JsonValue,
  ToolExecutionEnvelope,
  ToolInvocationContext,
} from "../src/contracts.js";
import { ToolResultForwarder } from "../src/ipc_forwarder.js";

export const MCP_AUTH_TOOL_NAME = "mcp_auth";

export const MCP_AUTH_INPUT_SCHEMA: {
  type: "object";
  properties: Record<string, object>;
  required: string[];
} = {
  type: "object",
  properties: {
    server: {
      type: "string",
      description: "Configured MCP server name.",
    },
    action: {
      type: "string",
      enum: ["status", "set_token", "clear_token"],
      description: "Auth action to perform for the target server.",
    },
    accessToken: {
      type: "string",
      description: "Bearer token to store when action is set_token.",
    },
  },
  required: ["server", "action"],
};

export interface McpServerAuthConfig {
  auth?: {
    bearerToken?: string;
    envVar?: string;
  };
}

export interface McpAuthTokenRecord {
  accessToken: string;
  updatedAt: string;
}

export interface McpAuthStore {
  getToken(server: string): McpAuthTokenRecord | undefined;
  setToken(server: string, accessToken: string): McpAuthTokenRecord;
  clearToken(server: string): boolean;
}

export class InMemoryMcpAuthStore implements McpAuthStore {
  private readonly tokens = new Map<string, McpAuthTokenRecord>();

  getToken(server: string): McpAuthTokenRecord | undefined {
    return this.tokens.get(server);
  }

  setToken(server: string, accessToken: string): McpAuthTokenRecord {
    const record = {
      accessToken,
      updatedAt: new Date().toISOString(),
    };
    this.tokens.set(server, record);
    return record;
  }

  clearToken(server: string): boolean {
    return this.tokens.delete(server);
  }
}

export type ResolvedAccessTokenSource = "memory" | "config" | "environment" | "none";

export interface ResolvedAccessToken {
  accessToken?: string;
  source: ResolvedAccessTokenSource;
}

export function resolveAccessToken(
  server: string,
  config: McpServerAuthConfig | undefined,
  store: McpAuthStore,
  env: NodeJS.ProcessEnv = process.env,
): ResolvedAccessToken {
  const stored = store.getToken(server);
  if (stored?.accessToken) {
    return {
      accessToken: stored.accessToken,
      source: "memory",
    };
  }

  const envVar = config?.auth?.envVar;
  if (envVar && env[envVar]) {
    return {
      accessToken: env[envVar],
      source: "environment",
    };
  }

  if (config?.auth?.bearerToken) {
    return {
      accessToken: config.auth.bearerToken,
      source: "config",
    };
  }

  return {
    source: "none",
  };
}

export interface McpAuthToolInput {
  server: string;
  action: "status" | "set_token" | "clear_token";
  accessToken?: string;
}

export interface McpAuthToolOutput {
  tool: "McpAuthTool";
  server: string;
  action: McpAuthToolInput["action"];
  configured: boolean;
  connected: boolean;
  hasToken: boolean;
  tokenSource: ResolvedAccessTokenSource;
  updatedAt?: string;
  message: string;
}

export interface McpAuthToolOptions {
  store: McpAuthStore;
  getServerConfig(server: string): McpServerAuthConfig | undefined;
  isConnected?(server: string): boolean;
  onCredentialsChanged?(server: string): Promise<void> | void;
}

export class McpAuthTool {
  constructor(private readonly options: McpAuthToolOptions) {}

  async execute(
    input: McpAuthToolInput,
    context?: ToolInvocationContext,
    forwarder?: ToolResultForwarder,
  ): Promise<ToolExecutionEnvelope<McpAuthToolOutput>> {
    const output = await this.run(input);
    if (context && forwarder) {
      await forwarder.forward(context, output);
    }
    return {
      tool: "McpAuthTool",
      ok: true,
      output,
    };
  }

  private async run(input: McpAuthToolInput): Promise<McpAuthToolOutput> {
    const server = requireNonEmptyString(input.server, "server");
    const configured = Boolean(this.options.getServerConfig(server));
    const connected = this.options.isConnected?.(server) ?? false;

    if (input.action === "set_token") {
      const accessToken = requireNonEmptyString(
        input.accessToken,
        "accessToken",
      );
      const record = this.options.store.setToken(server, accessToken);
      await this.options.onCredentialsChanged?.(server);
      return {
        tool: "McpAuthTool",
        server,
        action: "set_token",
        configured,
        connected,
        hasToken: true,
        tokenSource: "memory",
        updatedAt: record.updatedAt,
        message: `Stored bearer token for MCP server "${server}".`,
      };
    }

    if (input.action === "clear_token") {
      this.options.store.clearToken(server);
      await this.options.onCredentialsChanged?.(server);
      return {
        tool: "McpAuthTool",
        server,
        action: "clear_token",
        configured,
        connected: this.options.isConnected?.(server) ?? false,
        hasToken: false,
        tokenSource: "none",
        message: `Cleared stored bearer token for MCP server "${server}".`,
      };
    }

    const resolved = resolveAccessToken(
      server,
      this.options.getServerConfig(server),
      this.options.store,
    );
    return {
      tool: "McpAuthTool",
      server,
      action: "status",
      configured,
      connected,
      hasToken: Boolean(resolved.accessToken),
      tokenSource: resolved.source,
      updatedAt: this.options.store.getToken(server)?.updatedAt,
      message: configured
        ? `MCP auth status for "${server}" is ${resolved.source}.`
        : `MCP server "${server}" is not configured.`,
    };
  }
}

export function toJsonText(value: JsonValue): string {
  return JSON.stringify(value, null, 2);
}

function requireNonEmptyString(value: unknown, fieldName: string): string {
  if (typeof value !== "string" || value.trim() === "") {
    throw new Error(`Expected "${fieldName}" to be a non-empty string.`);
  }
  return value.trim();
}
