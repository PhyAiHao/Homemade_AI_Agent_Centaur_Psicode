import { stat, readFile } from "node:fs/promises";
import { extname, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import type {
  JsonValue,
  ToolExecutionEnvelope,
  ToolInvocationContext,
} from "../src/contracts.js";
import { ToolResultForwarder } from "../src/ipc_forwarder.js";
import type { DrainedLspDiagnostics } from "./diagnostic_registry.js";
import type { LspManager } from "./manager.js";

const MAX_LSP_FILE_SIZE_BYTES = 10_000_000;

export const LSP_TOOL_NAME = "lsp_query";

export const LSP_TOOL_INPUT_SCHEMA: {
  type: "object";
  properties: Record<string, object>;
  required: string[];
} = {
  type: "object",
  properties: {
    operation: {
      type: "string",
      enum: [
        "goToDefinition",
        "findReferences",
        "hover",
        "documentSymbol",
        "workspaceSymbol",
        "goToImplementation",
        "prepareCallHierarchy",
        "incomingCalls",
        "outgoingCalls",
      ],
      description: "LSP operation to perform.",
    },
    filePath: {
      type: "string",
      description: "Absolute or relative path to the target file.",
    },
    line: {
      type: "number",
      description: "1-based line number.",
    },
    character: {
      type: "number",
      description: "1-based character offset.",
    },
    query: {
      type: "string",
      description: "Optional workspace-symbol query override.",
    },
  },
  required: ["operation", "filePath", "line", "character"],
};

export type LspToolOperation =
  | "goToDefinition"
  | "findReferences"
  | "hover"
  | "documentSymbol"
  | "workspaceSymbol"
  | "goToImplementation"
  | "prepareCallHierarchy"
  | "incomingCalls"
  | "outgoingCalls";

export interface LspToolInput {
  operation: LspToolOperation;
  filePath: string;
  line: number;
  character: number;
  query?: string;
}

export interface LspToolOutput {
  tool: "LSPTool";
  operation: LspToolOperation;
  filePath: string;
  result: string;
  resultCount?: number;
  fileCount?: number;
  diagnostics?: JsonValue[];
}

export class LspTool {
  constructor(private readonly manager: LspManager) {}

  async execute(
    input: LspToolInput,
    context?: ToolInvocationContext,
    forwarder?: ToolResultForwarder,
  ): Promise<ToolExecutionEnvelope<LspToolOutput>> {
    const output = await this.run(input);
    if (context && forwarder) {
      await forwarder.forward(context, output);
    }
    return {
      tool: "LSPTool",
      ok: true,
      output,
    };
  }

  private async run(input: LspToolInput): Promise<LspToolOutput> {
    validateInput(input);
    const absolutePath = resolve(input.filePath);
    const fileStats = await stat(absolutePath);
    if (!fileStats.isFile()) {
      throw new Error(`Path is not a file: ${input.filePath}`);
    }
    if (fileStats.size > MAX_LSP_FILE_SIZE_BYTES) {
      return {
        tool: "LSPTool",
        operation: input.operation,
        filePath: input.filePath,
        result: `File too large for LSP analysis (${Math.ceil(fileStats.size / 1_000_000)}MB exceeds 10MB limit).`,
      };
    }

    if (!this.manager.isFileOpen(absolutePath)) {
      const content = await readFile(absolutePath, "utf8");
      await this.manager.openFile(absolutePath, content);
    }

    const request = buildRequest(input, absolutePath);
    let result = await this.manager.sendRequest<unknown>(
      absolutePath,
      request.method,
      request.params,
    );

    if (result === undefined) {
      return {
        tool: "LSPTool",
        operation: input.operation,
        filePath: input.filePath,
        result: `No LSP server available for file type: ${extname(absolutePath)}`,
      };
    }

    if (input.operation === "incomingCalls" || input.operation === "outgoingCalls") {
      const items = Array.isArray(result) ? result : [];
      if (items.length === 0) {
        return {
          tool: "LSPTool",
          operation: input.operation,
          filePath: input.filePath,
          result: "No call hierarchy item found at this position.",
          resultCount: 0,
          fileCount: 0,
        };
      }
      const method =
        input.operation === "incomingCalls"
          ? "callHierarchy/incomingCalls"
          : "callHierarchy/outgoingCalls";
      result = await this.manager.sendRequest<unknown>(absolutePath, method, {
        item: items[0],
      });
    }

    const formatted = formatResult(
      input.operation,
      result,
      process.cwd(),
      absolutePath,
    );
    const diagnostics = this.manager.drainPendingDiagnostics();
    return {
      tool: "LSPTool",
      operation: input.operation,
      filePath: input.filePath,
      result: formatted.result,
      ...(formatted.resultCount !== undefined ? { resultCount: formatted.resultCount } : {}),
      ...(formatted.fileCount !== undefined ? { fileCount: formatted.fileCount } : {}),
      ...(diagnostics.length > 0
        ? { diagnostics: diagnostics.map(item => toJsonDiagnosticBatch(item)) }
        : {}),
    };
  }
}

function buildRequest(
  input: LspToolInput,
  absolutePath: string,
): {
  method: string;
  params: Record<string, unknown>;
} {
  const uri = pathToFileURL(absolutePath).href;
  const position = {
    line: input.line - 1,
    character: input.character - 1,
  };

  switch (input.operation) {
    case "goToDefinition":
      return {
        method: "textDocument/definition",
        params: {
          textDocument: { uri },
          position,
        },
      };
    case "findReferences":
      return {
        method: "textDocument/references",
        params: {
          textDocument: { uri },
          position,
          context: { includeDeclaration: true },
        },
      };
    case "hover":
      return {
        method: "textDocument/hover",
        params: {
          textDocument: { uri },
          position,
        },
      };
    case "documentSymbol":
      return {
        method: "textDocument/documentSymbol",
        params: {
          textDocument: { uri },
        },
      };
    case "workspaceSymbol":
      return {
        method: "workspace/symbol",
        params: {
          query: input.query ?? "",
        },
      };
    case "goToImplementation":
      return {
        method: "textDocument/implementation",
        params: {
          textDocument: { uri },
          position,
        },
      };
    case "prepareCallHierarchy":
    case "incomingCalls":
    case "outgoingCalls":
      return {
        method: "textDocument/prepareCallHierarchy",
        params: {
          textDocument: { uri },
          position,
        },
      };
  }
}

function validateInput(input: LspToolInput): void {
  if (!input.filePath || typeof input.filePath !== "string") {
    throw new Error('Expected "filePath" to be a non-empty string.');
  }
  if (!Number.isInteger(input.line) || input.line <= 0) {
    throw new Error('Expected "line" to be a positive integer.');
  }
  if (!Number.isInteger(input.character) || input.character <= 0) {
    throw new Error('Expected "character" to be a positive integer.');
  }
}

function formatResult(
  operation: LspToolOperation,
  result: unknown,
  cwd: string,
  filePath: string,
): {
  result: string;
  resultCount?: number;
  fileCount?: number;
} {
  switch (operation) {
    case "goToDefinition":
    case "goToImplementation": {
      const locations = normalizeLocationArray(result);
      if (locations.length === 0) {
        return {
          result: "No definition found.",
          resultCount: 0,
          fileCount: 0,
        };
      }
      if (locations.length === 1) {
        return {
          result: `Defined in ${formatLocation(locations[0], cwd)}`,
          resultCount: 1,
          fileCount: 1,
        };
      }
      return {
        result: `Found ${locations.length} locations:\n${locations
          .map(location => `  ${formatLocation(location, cwd)}`)
          .join("\n")}`,
        resultCount: locations.length,
        fileCount: countDistinctUris(locations),
      };
    }
    case "findReferences": {
      const locations = normalizeLocationArray(result);
      if (locations.length === 0) {
        return {
          result: "No references found.",
          resultCount: 0,
          fileCount: 0,
        };
      }
      const grouped = groupLocationsByFile(locations, cwd);
      return {
        result: `Found ${locations.length} references across ${grouped.size} files:\n${[
          ...grouped.entries(),
        ]
          .map(([file, fileLocations]) =>
            `${file}:\n${fileLocations
              .map(location => {
                const line = location.range.start.line + 1;
                const character = location.range.start.character + 1;
                return `  Line ${line}:${character}`;
              })
              .join("\n")}`,
          )
          .join("\n")}`,
        resultCount: locations.length,
        fileCount: grouped.size,
      };
    }
    case "hover": {
      const content = normalizeHoverText(result);
      return {
        result: content || "No hover information available.",
      };
    }
    case "documentSymbol": {
      const lines = formatDocumentSymbols(result, cwd, filePath);
      return {
        result: lines.text,
        resultCount: lines.count,
        fileCount: 1,
      };
    }
    case "workspaceSymbol": {
      const symbols = Array.isArray(result) ? result : [];
      if (symbols.length === 0) {
        return {
          result: "No workspace symbols found.",
          resultCount: 0,
          fileCount: 0,
        };
      }
      const lines = symbols.map(symbol => {
        const name = getString(symbol, "name") ?? "<unnamed>";
        const location = isRecord(symbol.location)
          ? normalizeLocation(symbol.location)
          : null;
        return `- ${name}${location ? ` (${formatLocation(location, cwd)})` : ""}`;
      });
      return {
        result: `Found ${symbols.length} workspace symbols:\n${lines.join("\n")}`,
        resultCount: symbols.length,
        fileCount: countDistinctUris(
          symbols
            .map(symbol =>
              isRecord(symbol.location) ? normalizeLocation(symbol.location) : null,
            )
            .filter((value): value is NormalizedLocation => value !== null),
        ),
      };
    }
    case "prepareCallHierarchy": {
      const items = Array.isArray(result) ? result : [];
      if (items.length === 0) {
        return {
          result: "No call hierarchy item found.",
          resultCount: 0,
          fileCount: 0,
        };
      }
      const lines = items.map(item => {
        const name = getString(item, "name") ?? "<unnamed>";
        const uri = isRecord(item.uri) ? undefined : getString(item, "uri");
        return `- ${name}${uri ? ` (${shortenPath(uri, cwd)})` : ""}`;
      });
      return {
        result: `Prepared ${items.length} call hierarchy item(s):\n${lines.join("\n")}`,
        resultCount: items.length,
        fileCount: 1,
      };
    }
    case "incomingCalls":
    case "outgoingCalls": {
      const calls = Array.isArray(result) ? result : [];
      if (calls.length === 0) {
        return {
          result: "No call hierarchy results found.",
          resultCount: 0,
          fileCount: 0,
        };
      }
      const lines = calls.map(call => {
        const item = isRecord(call.from) ? call.from : isRecord(call.to) ? call.to : null;
        const name = item ? getString(item, "name") ?? "<unnamed>" : "<unknown>";
        const uri = item ? getString(item, "uri") : undefined;
        return `- ${name}${uri ? ` (${shortenPath(uri, cwd)})` : ""}`;
      });
      return {
        result: `Found ${calls.length} ${operation} result(s):\n${lines.join("\n")}`,
        resultCount: calls.length,
        fileCount: countDistinctUris(
          calls
            .map(call => {
              const item = isRecord(call.from)
                ? call.from
                : isRecord(call.to)
                  ? call.to
                  : null;
              const uri = item ? getString(item, "uri") : undefined;
              return uri
                ? {
                    uri,
                    range: {
                      start: { line: 0, character: 0 },
                      end: { line: 0, character: 0 },
                    },
                  }
                : null;
            })
            .filter((value): value is NormalizedLocation => value !== null),
        ),
      };
    }
  }
}

interface NormalizedLocation {
  uri: string;
  range: {
    start: { line: number; character: number };
    end: { line: number; character: number };
  };
}

function normalizeLocationArray(result: unknown): NormalizedLocation[] {
  if (!result) {
    return [];
  }
  if (Array.isArray(result)) {
    return result
      .map(item => normalizeLocation(item))
      .filter((value): value is NormalizedLocation => value !== null);
  }
  const single = normalizeLocation(result);
  return single ? [single] : [];
}

function normalizeLocation(value: unknown): NormalizedLocation | null {
  if (!isRecord(value)) {
    return null;
  }
  if (typeof value.uri === "string" && isRange(value.range)) {
    return {
      uri: value.uri,
      range: value.range,
    };
  }
  if (typeof value.targetUri === "string" && isRange(value.targetRange)) {
    return {
      uri: value.targetUri,
      range: value.targetSelectionRange && isRange(value.targetSelectionRange)
        ? value.targetSelectionRange
        : value.targetRange,
    };
  }
  return null;
}

function formatLocation(location: NormalizedLocation, cwd: string): string {
  return `${shortenPath(location.uri, cwd)}:${location.range.start.line + 1}:${location.range.start.character + 1}`;
}

function shortenPath(uri: string, cwd: string): string {
  const filePath = uri.startsWith("file://") ? filePathFromUri(uri) : uri;
  const relativePath = relative(cwd, filePath).replaceAll("\\", "/");
  return relativePath && !relativePath.startsWith("..") ? relativePath : filePath.replaceAll("\\", "/");
}

function filePathFromUri(uri: string): string {
  try {
    return uri.startsWith("file://") ? fileURLToPath(uri) : uri;
  } catch {
    return uri;
  }
}

function normalizeHoverText(result: unknown): string {
  if (!isRecord(result)) {
    return "";
  }
  const contents = result.contents;
  if (typeof contents === "string") {
    return contents;
  }
  if (Array.isArray(contents)) {
    return contents
      .map(item => {
        if (typeof item === "string") {
          return item;
        }
        if (isRecord(item) && typeof item.value === "string") {
          return item.value;
        }
        return "";
      })
      .filter(Boolean)
      .join("\n\n");
  }
  if (isRecord(contents) && typeof contents.value === "string") {
    return contents.value;
  }
  return "";
}

function formatDocumentSymbols(
  result: unknown,
  cwd: string,
  filePath: string,
): {
  text: string;
  count: number;
} {
  const symbols = Array.isArray(result) ? result : [];
  if (symbols.length === 0) {
    return {
      text: "No document symbols found.",
      count: 0,
    };
  }

  const lines: string[] = [];
  let count = 0;

  const visit = (symbol: unknown, depth: number): void => {
    if (!isRecord(symbol)) {
      return;
    }
    count += 1;
    const indent = "  ".repeat(depth);
    const name = getString(symbol, "name") ?? "<unnamed>";
    const range = isRange(symbol.selectionRange)
      ? symbol.selectionRange
      : isRange(symbol.range)
        ? symbol.range
        : undefined;
    const location = range
      ? `${relative(cwd, filePath).replaceAll("\\", "/") || filePath}:${range.start.line + 1}:${range.start.character + 1}`
      : "";
    lines.push(`- ${indent}${name}${location ? ` (${location})` : ""}`);
    if (Array.isArray(symbol.children)) {
      for (const child of symbol.children) {
        visit(child, depth + 1);
      }
    }
  };

  for (const symbol of symbols) {
    visit(symbol, 0);
  }

  return {
    text: `Found ${count} document symbol(s):\n${lines.join("\n")}`,
    count,
  };
}

function groupLocationsByFile(
  locations: NormalizedLocation[],
  cwd: string,
): Map<string, NormalizedLocation[]> {
  const grouped = new Map<string, NormalizedLocation[]>();
  for (const location of locations) {
    const file = shortenPath(location.uri, cwd);
    const entries = grouped.get(file) ?? [];
    entries.push(location);
    grouped.set(file, entries);
  }
  return grouped;
}

function countDistinctUris(locations: NormalizedLocation[]): number {
  return new Set(locations.map(location => location.uri)).size;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isRange(
  value: unknown,
): value is {
  start: { line: number; character: number };
  end: { line: number; character: number };
} {
  return (
    isRecord(value) &&
    isRecord(value.start) &&
    isRecord(value.end) &&
    typeof value.start.line === "number" &&
    typeof value.start.character === "number" &&
    typeof value.end.line === "number" &&
    typeof value.end.character === "number"
  );
}

function getString(
  value: unknown,
  key: string,
): string | undefined {
  return isRecord(value) && typeof value[key] === "string"
    ? (value[key] as string)
    : undefined;
}

function toJsonDiagnosticBatch(batch: DrainedLspDiagnostics): JsonValue {
  return {
    serverNames: batch.serverNames,
    totalDiagnostics: batch.totalDiagnostics,
    files: batch.files.map(file => ({
      uri: file.uri,
      diagnostics: file.diagnostics.map(diagnostic => ({
        message: diagnostic.message,
        severity: diagnostic.severity,
        range: diagnostic.range,
        ...(diagnostic.source ? { source: diagnostic.source } : {}),
        ...(diagnostic.code ? { code: diagnostic.code } : {}),
      })),
    })),
  };
}
