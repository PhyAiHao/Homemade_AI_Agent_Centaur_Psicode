import { fileURLToPath } from "node:url";
import { LspDiagnosticRegistry, type LspDiagnosticEntry, type LspDiagnosticFile, type LspDiagnosticSeverity } from "./diagnostic_registry.js";
import type { LspManager } from "./manager.js";

export interface PublishDiagnosticsParams {
  uri: string;
  diagnostics: Array<{
    message: string;
    severity?: number;
    range: {
      start: {
        line: number;
        character: number;
      };
      end: {
        line: number;
        character: number;
      };
    };
    source?: string;
    code?: string | number;
  }>;
}

export interface LspDiagnosticEvent {
  type: "lsp_diagnostics";
  serverName: string;
  files: LspDiagnosticFile[];
  totalDiagnostics: number;
  timestamp: string;
}

export interface LspDiagnosticEventWriter {
  write(event: LspDiagnosticEvent): Promise<void> | void;
}

export class MemoryLspDiagnosticEventWriter
  implements LspDiagnosticEventWriter
{
  readonly events: LspDiagnosticEvent[] = [];

  write(event: LspDiagnosticEvent): void {
    this.events.push(event);
  }
}

export interface PassiveFeedbackRegistration {
  totalServers: number;
  successCount: number;
  registrationErrors: Array<{
    serverName: string;
    error: string;
  }>;
}

export function registerLspNotificationHandlers(
  manager: LspManager,
  registry: LspDiagnosticRegistry,
  writer?: LspDiagnosticEventWriter,
): PassiveFeedbackRegistration {
  const servers = manager.getAllServers();
  const registrationErrors: PassiveFeedbackRegistration["registrationErrors"] = [];
  let successCount = 0;

  for (const [serverName, server] of servers.entries()) {
    try {
      server.onNotification(
        "textDocument/publishDiagnostics",
        async params => {
          if (!isPublishDiagnosticsParams(params)) {
            return;
          }

          const files = formatDiagnosticsForAttachment(params);
          if (files.length === 0) {
            return;
          }

          registry.register(serverName, files);
          const totalDiagnostics = files.reduce(
            (sum, file) => sum + file.diagnostics.length,
            0,
          );
          if (writer && totalDiagnostics > 0) {
            await writer.write({
              type: "lsp_diagnostics",
              serverName,
              files,
              totalDiagnostics,
              timestamp: new Date().toISOString(),
            });
          }
        },
      );
      successCount += 1;
    } catch (error) {
      registrationErrors.push({
        serverName,
        error: error instanceof Error ? error.message : String(error),
      });
    }
  }

  return {
    totalServers: servers.size,
    successCount,
    registrationErrors,
  };
}

export function formatDiagnosticsForAttachment(
  params: PublishDiagnosticsParams,
): LspDiagnosticFile[] {
  const uri = normalizeUriToPath(params.uri);
  const diagnostics = params.diagnostics.map<LspDiagnosticEntry>(diagnostic => ({
    message: diagnostic.message,
    severity: mapLspSeverity(diagnostic.severity),
    range: diagnostic.range,
    ...(diagnostic.source ? { source: diagnostic.source } : {}),
    ...(diagnostic.code !== undefined ? { code: String(diagnostic.code) } : {}),
  }));

  if (diagnostics.length === 0) {
    return [];
  }

  return [
    {
      uri,
      diagnostics,
    },
  ];
}

export function mapLspSeverity(severity: number | undefined): LspDiagnosticSeverity {
  switch (severity) {
    case 1:
      return "Error";
    case 2:
      return "Warning";
    case 3:
      return "Info";
    case 4:
      return "Hint";
    default:
      return "Error";
  }
}

function normalizeUriToPath(uri: string): string {
  if (!uri.startsWith("file://")) {
    return uri;
  }
  try {
    return fileURLToPath(uri);
  } catch {
    return uri;
  }
}

function isPublishDiagnosticsParams(
  params: unknown,
): params is PublishDiagnosticsParams {
  return (
    typeof params === "object" &&
    params !== null &&
    typeof (params as PublishDiagnosticsParams).uri === "string" &&
    Array.isArray((params as PublishDiagnosticsParams).diagnostics)
  );
}
