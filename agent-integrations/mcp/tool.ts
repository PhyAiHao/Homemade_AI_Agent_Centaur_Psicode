import type {
  JsonValue,
  ToolExecutionEnvelope,
  ToolInvocationContext,
} from "../src/contracts.js";
import { ToolResultForwarder } from "../src/ipc_forwarder.js";
import type { McpConnectionManager } from "./server.js";

const DEFAULT_BINARY_PREVIEW_CHARS = 4_096;

export const MCP_TOOL_NAME = "mcp_call_tool";
export const REMOTE_MCP_TOOL_PREFIX = "mcp:";

export const MCP_TOOL_INPUT_SCHEMA: {
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
    tool: {
      type: "string",
      description: "Remote MCP tool name.",
    },
    arguments: {
      type: "object",
      description: "JSON object passed to the remote MCP tool.",
    },
  },
  required: ["server", "tool"],
};

export interface McpToolInput {
  server: string;
  tool: string;
  arguments?: Record<string, JsonValue>;
}

export interface McpAdvertisedTool {
  server: string;
  remoteName: string;
  relayName: string;
  description?: string;
  title?: string;
  inputSchema: {
    type: "object";
    properties?: Record<string, object>;
    required?: string[];
  };
}

export interface McpToolOutput {
  tool: "MCPTool";
  server: string;
  name: string;
  isError: boolean;
  content: JsonValue[];
  structuredContent?: JsonValue;
  meta?: JsonValue;
}

export class McpToolInvoker {
  constructor(private readonly manager: McpConnectionManager) {}

  async execute(
    input: McpToolInput,
    context?: ToolInvocationContext,
    forwarder?: ToolResultForwarder,
  ): Promise<ToolExecutionEnvelope<McpToolOutput>> {
    const server = requireNonEmptyString(input.server, "server");
    const tool = requireNonEmptyString(input.tool, "tool");
    const result = await this.manager.callTool(server, tool, input.arguments ?? {});
    const contentBlocks = Array.isArray(result.content)
      ? (result.content as Record<string, unknown>[])
      : [];
    const output: McpToolOutput = {
      tool: "MCPTool",
      server,
      name: tool,
      isError: Boolean(result.isError),
      content: contentBlocks.map(block => normalizeContentBlock(block)),
      ...(result.structuredContent
        ? { structuredContent: toJsonSafe(result.structuredContent) }
        : {}),
      ...(result._meta ? { meta: toJsonSafe(result._meta) } : {}),
    };

    if (context && forwarder) {
      await forwarder.forward(context, output, output.isError);
    }

    return {
      tool: "MCPTool",
      ok: !output.isError,
      output,
    };
  }
}

export function encodeRemoteMcpToolName(server: string, toolName: string): string {
  return `${REMOTE_MCP_TOOL_PREFIX}${encodeURIComponent(server)}:${encodeURIComponent(toolName)}`;
}

export function decodeRemoteMcpToolName(relayName: string): {
  server: string;
  toolName: string;
} | null {
  if (!relayName.startsWith(REMOTE_MCP_TOOL_PREFIX)) {
    return null;
  }
  const encoded = relayName.slice(REMOTE_MCP_TOOL_PREFIX.length);
  const separatorIndex = encoded.indexOf(":");
  if (separatorIndex <= 0) {
    return null;
  }
  return {
    server: decodeURIComponent(encoded.slice(0, separatorIndex)),
    toolName: decodeURIComponent(encoded.slice(separatorIndex + 1)),
  };
}

function normalizeContentBlock(block: Record<string, unknown>): JsonValue {
  if (block.type === "text" && typeof block.text === "string") {
    return {
      type: "text",
      text: block.text,
    };
  }

  if (
    (block.type === "image" || block.type === "audio") &&
    typeof block.mimeType === "string" &&
    typeof block.data === "string"
  ) {
    const truncated = block.data.length > DEFAULT_BINARY_PREVIEW_CHARS;
    return {
      type: block.type,
      mimeType: block.mimeType,
      dataPreview: truncated
        ? `${block.data.slice(0, DEFAULT_BINARY_PREVIEW_CHARS)}...`
        : block.data,
      truncated,
    };
  }

  if (
    block.type === "resource" &&
    typeof block.resource === "object" &&
    block.resource !== null
  ) {
    const resource = block.resource as Record<string, unknown>;
    if (typeof resource.text === "string") {
      return {
        type: "resource",
        resource: {
          uri: String(resource.uri ?? ""),
          ...(resource.mimeType ? { mimeType: String(resource.mimeType) } : {}),
          text: resource.text,
        },
      };
    }
    if (typeof resource.blob === "string") {
      const truncated = resource.blob.length > DEFAULT_BINARY_PREVIEW_CHARS;
      return {
        type: "resource",
        resource: {
          uri: String(resource.uri ?? ""),
          ...(resource.mimeType ? { mimeType: String(resource.mimeType) } : {}),
          blobPreview: truncated
            ? `${resource.blob.slice(0, DEFAULT_BINARY_PREVIEW_CHARS)}...`
            : resource.blob,
          truncated,
        },
      };
    }
  }

  return toJsonSafe(block);
}

function requireNonEmptyString(value: unknown, fieldName: string): string {
  if (typeof value !== "string" || value.trim() === "") {
    throw new Error(`Expected "${fieldName}" to be a non-empty string.`);
  }
  return value.trim();
}

function toJsonSafe(value: unknown): JsonValue {
  if (
    value === null ||
    typeof value === "string" ||
    typeof value === "number" ||
    typeof value === "boolean"
  ) {
    return value;
  }
  if (Array.isArray(value)) {
    return value.map(item => toJsonSafe(item));
  }
  if (typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => [key, toJsonSafe(item)]),
    );
  }
  return String(value);
}
