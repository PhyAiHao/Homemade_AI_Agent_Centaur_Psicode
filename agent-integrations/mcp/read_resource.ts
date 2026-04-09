import type {
  JsonValue,
  ToolExecutionEnvelope,
  ToolInvocationContext,
} from "../src/contracts.js";
import { ToolResultForwarder } from "../src/ipc_forwarder.js";
import type { McpConnectionManager } from "./server.js";

const DEFAULT_BLOB_PREVIEW_CHARS = 4_096;

export const READ_MCP_RESOURCE_TOOL_NAME = "mcp_read_resource";

export const READ_MCP_RESOURCE_INPUT_SCHEMA: {
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
    uri: {
      type: "string",
      description: "Absolute MCP resource URI to read.",
    },
  },
  required: ["server", "uri"],
};

export interface ReadMcpResourceInput {
  server: string;
  uri: string;
}

export interface ReadMcpResourceOutput {
  tool: "ReadMcpResource";
  server: string;
  uri: string;
  contents: JsonValue[];
}

export class ReadMcpResourceTool {
  constructor(private readonly manager: McpConnectionManager) {}

  async execute(
    input: ReadMcpResourceInput,
    context?: ToolInvocationContext,
    forwarder?: ToolResultForwarder,
  ): Promise<ToolExecutionEnvelope<ReadMcpResourceOutput>> {
    const server = requireNonEmptyString(input.server, "server");
    const uri = requireNonEmptyString(input.uri, "uri");
    const result = await this.manager.readResource(server, uri);
    const output: ReadMcpResourceOutput = {
      tool: "ReadMcpResource",
      server,
      uri,
      contents: result.contents.map(content => normalizeResourceContent(content)),
    };

    if (context && forwarder) {
      await forwarder.forward(context, output);
    }

    return {
      tool: "ReadMcpResource",
      ok: true,
      output,
    };
  }
}

function normalizeResourceContent(content: {
  uri: string;
  mimeType?: string;
  text?: string;
  blob?: string;
  _meta?: Record<string, unknown>;
}): JsonValue {
  const base = {
    uri: content.uri,
    ...(content.mimeType ? { mimeType: content.mimeType } : {}),
    ...(content._meta ? { meta: toJsonSafe(content._meta) } : {}),
  };

  if (typeof content.text === "string") {
    return {
      ...base,
      text: content.text,
    };
  }

  if (typeof content.blob === "string") {
    const truncated = content.blob.length > DEFAULT_BLOB_PREVIEW_CHARS;
    return {
      ...base,
      blobPreview: truncated
        ? `${content.blob.slice(0, DEFAULT_BLOB_PREVIEW_CHARS)}...`
        : content.blob,
      truncated,
    };
  }

  return base;
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
