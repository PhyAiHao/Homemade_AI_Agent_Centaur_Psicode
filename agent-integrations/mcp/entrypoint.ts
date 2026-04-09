import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { pathToFileURL } from "node:url";
import { createMcpRelayServer, McpConnectionManager, type McpServerConfig } from "./server.js";

export interface StartMcpEntrypointOptions {
  servers: McpServerConfig[];
  name?: string;
  version?: string;
  instructions?: string;
}

export async function startMcpEntrypoint(
  options: StartMcpEntrypointOptions,
): Promise<void> {
  const manager = new McpConnectionManager({
    servers: options.servers,
    clientInfo: {
      name: options.name ?? "centaur/mcp-relay",
      version: options.version ?? "0.1.0",
    },
  });
  const server = createMcpRelayServer({
    manager,
    name: options.name,
    version: options.version,
    instructions: options.instructions,
  });
  const transport = new StdioServerTransport();
  await server.connect(transport);
}

export function loadMcpConfigsFromEnv(
  env: NodeJS.ProcessEnv = process.env,
): McpServerConfig[] {
  const raw = env.CENTAUR_MCP_CONFIG_JSON;
  if (!raw) {
    return [];
  }
  const parsed = JSON.parse(raw) as unknown;
  if (!Array.isArray(parsed)) {
    throw new Error("CENTAUR_MCP_CONFIG_JSON must be a JSON array.");
  }
  return parsed as McpServerConfig[];
}

function isDirectExecution(): boolean {
  const entryPath = process.argv[1];
  if (!entryPath) {
    return false;
  }
  return import.meta.url === pathToFileURL(entryPath).href;
}

if (isDirectExecution()) {
  const servers = loadMcpConfigsFromEnv();
  if (servers.length === 0) {
    throw new Error(
      "No MCP servers configured. Set CENTAUR_MCP_CONFIG_JSON to a JSON array of server configs.",
    );
  }
  await startMcpEntrypoint({
    servers,
    name: process.env.CENTAUR_MCP_SERVER_NAME,
    version: process.env.CENTAUR_MCP_SERVER_VERSION,
    instructions: process.env.CENTAUR_MCP_SERVER_INSTRUCTIONS,
  });
}
