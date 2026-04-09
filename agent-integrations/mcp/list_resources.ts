import type {
  ToolExecutionEnvelope,
  ToolInvocationContext,
} from "../src/contracts.js";
import { ToolResultForwarder } from "../src/ipc_forwarder.js";
import type { McpConnectionManager } from "./server.js";

export const LIST_MCP_RESOURCES_TOOL_NAME = "mcp_list_resources";

export const LIST_MCP_RESOURCES_INPUT_SCHEMA: {
  type: "object";
  properties: Record<string, object>;
} = {
  type: "object",
  properties: {
    server: {
      type: "string",
      description: "Optional MCP server name to limit the resource list.",
    },
    cursor: {
      type: "string",
      description: "Optional pagination cursor to continue a previous listing.",
    },
  },
};

export interface ListMcpResourcesInput {
  server?: string;
  cursor?: string;
}

export interface ListedMcpResource {
  server: string;
  uri: string;
  name: string;
  description?: string;
  mimeType?: string;
  size?: number;
  title?: string;
}

export interface ListMcpResourcesOutput {
  tool: "ListMcpResources";
  server?: string;
  count: number;
  resources: ListedMcpResource[];
  nextCursor?: string;
  nextCursorByServer?: Record<string, string>;
  errors?: Array<{
    server: string;
    message: string;
  }>;
}

export class ListMcpResourcesTool {
  constructor(private readonly manager: McpConnectionManager) {}

  async execute(
    input: ListMcpResourcesInput,
    context?: ToolInvocationContext,
    forwarder?: ToolResultForwarder,
  ): Promise<ToolExecutionEnvelope<ListMcpResourcesOutput>> {
    const result = await this.manager.listResources(input.server, input.cursor);
    const output: ListMcpResourcesOutput = {
      tool: "ListMcpResources",
      ...(input.server ? { server: input.server } : {}),
      count: result.resources.length,
      resources: result.resources.map(resource => ({
        server: resource.server,
        uri: resource.uri,
        name: resource.name,
        ...(resource.description ? { description: resource.description } : {}),
        ...(resource.mimeType ? { mimeType: resource.mimeType } : {}),
        ...(typeof resource.size === "number" ? { size: resource.size } : {}),
        ...(resource.title ? { title: resource.title } : {}),
      })),
      ...(result.nextCursor ? { nextCursor: result.nextCursor } : {}),
      ...(Object.keys(result.nextCursorByServer).length > 0
        ? { nextCursorByServer: result.nextCursorByServer }
        : {}),
      ...(result.errors.length > 0 ? { errors: result.errors } : {}),
    };

    if (context && forwarder) {
      await forwarder.forward(context, output, result.errors.length > 0);
    }

    return {
      tool: "ListMcpResources",
      ok: result.errors.length === 0,
      output,
    };
  }
}
