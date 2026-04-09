import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import {
  SSEClientTransport,
  type SSEClientTransportOptions,
} from "@modelcontextprotocol/sdk/client/sse.js";
import {
  StdioClientTransport,
  type StdioServerParameters,
} from "@modelcontextprotocol/sdk/client/stdio.js";
import {
  StreamableHTTPClientTransport,
  type StreamableHTTPClientTransportOptions,
} from "@modelcontextprotocol/sdk/client/streamableHttp.js";
import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
  type CallToolResult,
  type ReadResourceResult,
  type Tool,
} from "@modelcontextprotocol/sdk/types.js";
import {
  InMemoryMcpAuthStore,
  MCP_AUTH_INPUT_SCHEMA,
  MCP_AUTH_TOOL_NAME,
  type McpAuthStore,
  type McpServerAuthConfig,
  McpAuthTool,
  resolveAccessToken,
} from "./auth_tool.js";
import {
  LIST_MCP_RESOURCES_INPUT_SCHEMA,
  LIST_MCP_RESOURCES_TOOL_NAME,
  ListMcpResourcesTool,
} from "./list_resources.js";
import {
  READ_MCP_RESOURCE_INPUT_SCHEMA,
  READ_MCP_RESOURCE_TOOL_NAME,
  ReadMcpResourceTool,
} from "./read_resource.js";
import {
  decodeRemoteMcpToolName,
  encodeRemoteMcpToolName,
  MCP_TOOL_INPUT_SCHEMA,
  MCP_TOOL_NAME,
  McpToolInvoker,
} from "./tool.js";

type FetchLike = typeof fetch;

export interface McpServerConfig extends McpServerAuthConfig {
  name: string;
  transport: "stdio" | "sse" | "http";
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  cwd?: string;
  url?: string;
  headers?: Record<string, string>;
}

export interface McpListedResource {
  server: string;
  uri: string;
  name: string;
  description?: string;
  mimeType?: string;
  size?: number;
  title?: string;
}

export interface McpListResourcesResult {
  resources: McpListedResource[];
  nextCursor?: string;
  nextCursorByServer: Record<string, string>;
  errors: Array<{
    server: string;
    message: string;
  }>;
}

export interface McpListedTool {
  server: string;
  remoteName: string;
  relayName: string;
  description?: string;
  title?: string;
  inputSchema: Tool["inputSchema"];
}

interface ManagedConnection {
  readonly config: McpServerConfig;
  readonly client: Client;
}

export interface McpConnectionManagerOptions {
  servers: McpServerConfig[];
  authStore?: McpAuthStore;
  fetchImpl?: FetchLike;
  clientInfo?: {
    name: string;
    version: string;
  };
}

export interface McpRelayServerOptions {
  manager: McpConnectionManager;
  authTool?: McpAuthTool;
  name?: string;
  version?: string;
  instructions?: string;
}

const DEFAULT_SERVER_NAME = "centaur/mcp-relay";
const DEFAULT_SERVER_VERSION = "0.1.0";

export class McpConnectionManager {
  private readonly configs = new Map<string, McpServerConfig>();
  private readonly connections = new Map<string, ManagedConnection>();
  private readonly authStore: McpAuthStore;
  private readonly fetchImpl?: FetchLike;
  private readonly clientInfo: {
    name: string;
    version: string;
  };

  constructor(options: McpConnectionManagerOptions) {
    for (const server of options.servers) {
      this.configs.set(server.name, server);
    }
    this.authStore = options.authStore ?? new InMemoryMcpAuthStore();
    this.fetchImpl = options.fetchImpl;
    this.clientInfo = options.clientInfo ?? {
      name: DEFAULT_SERVER_NAME,
      version: DEFAULT_SERVER_VERSION,
    };
  }

  getAuthStore(): McpAuthStore {
    return this.authStore;
  }

  getConfig(name: string): McpServerConfig | undefined {
    return this.configs.get(name);
  }

  listServerNames(): string[] {
    return [...this.configs.keys()].sort();
  }

  isConnected(name: string): boolean {
    return this.connections.has(name);
  }

  async resetConnection(name: string): Promise<void> {
    await this.disconnect(name);
  }

  async disconnect(name: string): Promise<void> {
    const connection = this.connections.get(name);
    if (!connection) {
      return;
    }
    this.connections.delete(name);
    await connection.client.close();
  }

  async closeAll(): Promise<void> {
    for (const name of [...this.connections.keys()]) {
      await this.disconnect(name);
    }
  }

  async connect(name: string): Promise<Client> {
    const existing = this.connections.get(name);
    if (existing) {
      return existing.client;
    }

    const config = this.requireConfig(name);
    const client = new Client(this.clientInfo, {
      capabilities: {},
    });
    const transport = this.createTransport(config);
    const clear = (): void => {
      this.connections.delete(name);
    };
    transport.onclose = clear;
    transport.onerror = () => {
      clear();
    };
    await client.connect(transport);
    this.connections.set(name, {
      config,
      client,
    });
    return client;
  }

  async listRemoteTools(serverName?: string): Promise<McpListedTool[]> {
    const names = serverName ? [serverName] : this.listServerNames();
    const tools: McpListedTool[] = [];

    for (const name of names) {
      const client = await this.connect(name);
      const result = await client.listTools();
      for (const tool of result.tools) {
        tools.push({
          server: name,
          remoteName: tool.name,
          relayName: encodeRemoteMcpToolName(name, tool.name),
          description: tool.description,
          title: tool.title,
          inputSchema: tool.inputSchema,
        });
      }
    }

    return tools.sort((left, right) => {
      return left.relayName.localeCompare(right.relayName);
    });
  }

  async callTool(
    serverName: string,
    toolName: string,
    args: Record<string, unknown>,
  ): Promise<Awaited<ReturnType<Client["callTool"]>>> {
    const client = await this.connect(serverName);
    return client.callTool({
      name: toolName,
      arguments: args,
    });
  }

  async listResources(
    serverName?: string,
    cursor?: string,
  ): Promise<McpListResourcesResult> {
    const names = serverName ? [serverName] : this.listServerNames();
    const resources: McpListedResource[] = [];
    const nextCursorByServer: Record<string, string> = {};
    const errors: Array<{
      server: string;
      message: string;
    }> = [];

    for (const name of names) {
      try {
        const client = await this.connect(name);
        const result = await client.listResources(cursor ? { cursor } : undefined);
        for (const resource of result.resources) {
          resources.push({
            server: name,
            uri: resource.uri,
            name: resource.name,
            ...(resource.description ? { description: resource.description } : {}),
            ...(resource.mimeType ? { mimeType: resource.mimeType } : {}),
            ...(typeof resource.size === "number" ? { size: resource.size } : {}),
            ...(resource.title ? { title: resource.title } : {}),
          });
        }
        if (result.nextCursor) {
          nextCursorByServer[name] = result.nextCursor;
        }
      } catch (error) {
        if (serverName) {
          throw error;
        }
        errors.push({
          server: name,
          message: error instanceof Error ? error.message : String(error),
        });
      }
    }

    return {
      resources,
      ...(serverName && nextCursorByServer[serverName]
        ? { nextCursor: nextCursorByServer[serverName] }
        : {}),
      nextCursorByServer,
      errors,
    };
  }

  async readResource(serverName: string, uri: string): Promise<ReadResourceResult> {
    const client = await this.connect(serverName);
    return client.readResource({
      uri,
    });
  }

  private requireConfig(name: string): McpServerConfig {
    const config = this.configs.get(name);
    if (!config) {
      throw new Error(
        `MCP server "${name}" is not configured. Available servers: ${this.listServerNames().join(", ")}`,
      );
    }
    return config;
  }

  private createTransport(config: McpServerConfig) {
    if (config.transport === "stdio") {
      const params: StdioServerParameters = {
        command: requireNonEmptyString(config.command, "command"),
        ...(config.args ? { args: config.args } : {}),
        ...(config.env ? { env: config.env } : {}),
        ...(config.cwd ? { cwd: config.cwd } : {}),
      };
      return new StdioClientTransport(params);
    }

    const headers = this.resolveHeaders(config);

    if (config.transport === "sse") {
      const sseOptions: SSEClientTransportOptions = {
        ...(Object.keys(headers).length > 0
          ? {
              requestInit: {
                headers,
              },
              eventSourceInit: {
                headers,
              } as NonNullable<SSEClientTransportOptions["eventSourceInit"]>,
            }
          : {}),
        ...(this.fetchImpl ? { fetch: this.fetchImpl } : {}),
      };
      return new SSEClientTransport(new URL(requireNonEmptyString(config.url, "url")), sseOptions);
    }

    const httpOptions: StreamableHTTPClientTransportOptions = {
      ...(Object.keys(headers).length > 0
        ? {
            requestInit: {
              headers,
            },
          }
        : {}),
      ...(this.fetchImpl ? { fetch: this.fetchImpl } : {}),
    };
    return new StreamableHTTPClientTransport(
      new URL(requireNonEmptyString(config.url, "url")),
      httpOptions,
    );
  }

  private resolveHeaders(config: McpServerConfig): Record<string, string> {
    const headers = new Headers(config.headers ?? {});
    const resolved = resolveAccessToken(config.name, config, this.authStore);
    if (resolved.accessToken) {
      headers.set("authorization", `Bearer ${resolved.accessToken}`);
    }
    return Object.fromEntries(headers.entries());
  }
}

export function createMcpRelayServer(options: McpRelayServerOptions): Server {
  const authTool =
    options.authTool ??
    new McpAuthTool({
      store: options.manager.getAuthStore(),
      getServerConfig: server => options.manager.getConfig(server),
      isConnected: server => options.manager.isConnected(server),
      onCredentialsChanged: async server => {
        await options.manager.resetConnection(server);
      },
    });
  const listResourcesTool = new ListMcpResourcesTool(options.manager);
  const readResourceTool = new ReadMcpResourceTool(options.manager);
  const mcpTool = new McpToolInvoker(options.manager);

  const server = new Server(
    {
      name: options.name ?? DEFAULT_SERVER_NAME,
      version: options.version ?? DEFAULT_SERVER_VERSION,
    },
    {
      capabilities: {
        tools: {},
      },
      ...(options.instructions ? { instructions: options.instructions } : {}),
    },
  );

  server.setRequestHandler(ListToolsRequestSchema, async (): Promise<{
    tools: Tool[];
  }> => {
    const remoteTools = await options.manager.listRemoteTools();
    return {
      tools: [
        createBuiltInTool({
          name: MCP_AUTH_TOOL_NAME,
          title: "MCP Auth",
          description: "Inspect, set, or clear bearer-token credentials for a configured MCP server.",
          inputSchema: MCP_AUTH_INPUT_SCHEMA,
          readOnlyHint: false,
        }),
        createBuiltInTool({
          name: LIST_MCP_RESOURCES_TOOL_NAME,
          title: "List MCP Resources",
          description: "List resources exposed by one or more configured MCP servers.",
          inputSchema: LIST_MCP_RESOURCES_INPUT_SCHEMA,
          readOnlyHint: true,
        }),
        createBuiltInTool({
          name: READ_MCP_RESOURCE_TOOL_NAME,
          title: "Read MCP Resource",
          description: "Read a specific MCP resource from a configured server.",
          inputSchema: READ_MCP_RESOURCE_INPUT_SCHEMA,
          readOnlyHint: true,
        }),
        createBuiltInTool({
          name: MCP_TOOL_NAME,
          title: "Call MCP Tool",
          description: "Call a remote MCP tool by explicit server and tool name.",
          inputSchema: MCP_TOOL_INPUT_SCHEMA,
          readOnlyHint: false,
          openWorldHint: true,
        }),
        ...remoteTools.map(tool =>
          createBuiltInTool({
            name: tool.relayName,
            title: tool.title ?? `${tool.server} / ${tool.remoteName}`,
            description: `[${tool.server}] ${tool.description ?? "Relayed MCP tool."}`,
            inputSchema: tool.inputSchema,
            readOnlyHint: false,
            openWorldHint: true,
          }),
        ),
      ],
    };
  });

  server.setRequestHandler(
    CallToolRequestSchema,
    async ({ params: { name, arguments: rawArgs } }): Promise<CallToolResult> => {
      try {
        const args = ensureObjectRecord(rawArgs);

        if (name === MCP_AUTH_TOOL_NAME) {
          const result = await authTool.execute({
            server: requireNonEmptyString(args.server, "server"),
            action: requireAction(args.action),
            ...(typeof args.accessToken === "string"
              ? { accessToken: args.accessToken }
              : {}),
          });
          return createCallToolResult(result.output, false);
        }

        if (name === LIST_MCP_RESOURCES_TOOL_NAME) {
          const result = await listResourcesTool.execute({
            ...(typeof args.server === "string" ? { server: args.server } : {}),
            ...(typeof args.cursor === "string" ? { cursor: args.cursor } : {}),
          });
          return createCallToolResult(result.output, !result.ok);
        }

        if (name === READ_MCP_RESOURCE_TOOL_NAME) {
          const result = await readResourceTool.execute({
            server: requireNonEmptyString(args.server, "server"),
            uri: requireNonEmptyString(args.uri, "uri"),
          });
          return createCallToolResult(result.output, false);
        }

        if (name === MCP_TOOL_NAME) {
          const result = await mcpTool.execute({
            server: requireNonEmptyString(args.server, "server"),
            tool: requireNonEmptyString(args.tool, "tool"),
            ...(isRecord(args.arguments)
              ? { arguments: toJsonRecord(args.arguments) }
              : {}),
          });
          return createCallToolResult(result.output, !result.ok);
        }

        const decoded = decodeRemoteMcpToolName(name);
        if (decoded) {
          const result = await mcpTool.execute({
            server: decoded.server,
            tool: decoded.toolName,
            arguments: toJsonRecord(args),
          });
          return createCallToolResult(result.output, !result.ok);
        }

        return createCallToolResult(
          {
            error: `Unknown MCP relay tool "${name}".`,
          },
          true,
        );
      } catch (error) {
        return createCallToolResult(
          {
            error: error instanceof Error ? error.message : String(error),
          },
          true,
        );
      }
    },
  );

  return server;
}

function createBuiltInTool(options: {
  name: string;
  title: string;
  description: string;
  inputSchema: Tool["inputSchema"];
  readOnlyHint: boolean;
  openWorldHint?: boolean;
}): Tool {
  return {
    name: options.name,
    title: options.title,
    description: options.description,
    inputSchema: options.inputSchema,
    annotations: {
      readOnlyHint: options.readOnlyHint,
      destructiveHint: false,
      idempotentHint: true,
      ...(options.openWorldHint ? { openWorldHint: true } : {}),
    },
  };
}

function createCallToolResult(output: unknown, isError: boolean): CallToolResult {
  const text =
    typeof output === "string" ? output : JSON.stringify(output, null, 2);
  return {
    isError,
    content: [
      {
        type: "text",
        text,
      },
    ],
    ...(isStructuredContent(output)
      ? { structuredContent: output }
      : Array.isArray(output)
        ? { structuredContent: { items: output } }
        : output === undefined
          ? {}
          : { structuredContent: { value: output } }),
  };
}

function ensureObjectRecord(value: unknown): Record<string, unknown> {
  if (value === undefined) {
    return {};
  }
  if (!isRecord(value)) {
    throw new Error("Expected tool arguments to be a JSON object.");
  }
  return value;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function toJsonRecord(
  record: Record<string, unknown>,
): Record<string, import("../src/contracts.js").JsonValue> {
  return Object.fromEntries(
    Object.entries(record).map(([key, value]) => [key, toJsonValue(value)]),
  );
}

function toJsonValue(value: unknown): import("../src/contracts.js").JsonValue {
  if (
    value === null ||
    typeof value === "string" ||
    typeof value === "number" ||
    typeof value === "boolean"
  ) {
    return value;
  }
  if (Array.isArray(value)) {
    return value.map(item => toJsonValue(item));
  }
  if (isRecord(value)) {
    return toJsonRecord(value);
  }
  return String(value);
}

function isStructuredContent(
  value: unknown,
): value is Record<string, unknown> {
  return isRecord(value);
}

function requireAction(value: unknown): "status" | "set_token" | "clear_token" {
  if (value === "status" || value === "set_token" || value === "clear_token") {
    return value;
  }
  throw new Error('Expected "action" to be one of: status, set_token, clear_token.');
}

function requireNonEmptyString(value: unknown, fieldName: string): string {
  if (typeof value !== "string" || value.trim() === "") {
    throw new Error(`Expected "${fieldName}" to be a non-empty string.`);
  }
  return value.trim();
}
