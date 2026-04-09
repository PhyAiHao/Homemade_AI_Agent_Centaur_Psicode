import { randomUUID } from "node:crypto";

const MAX_DIAGNOSTICS_PER_FILE = 10;
const MAX_TOTAL_DIAGNOSTICS = 30;
const MAX_TRACKED_FILES = 500;

export type LspDiagnosticSeverity = "Error" | "Warning" | "Info" | "Hint";

export interface LspDiagnosticEntry {
  message: string;
  severity: LspDiagnosticSeverity;
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
  code?: string;
}

export interface LspDiagnosticFile {
  uri: string;
  diagnostics: LspDiagnosticEntry[];
}

export interface PendingLspDiagnostic {
  id: string;
  serverName: string;
  files: LspDiagnosticFile[];
  timestamp: number;
}

export interface DrainedLspDiagnostics {
  serverNames: string[];
  files: LspDiagnosticFile[];
  totalDiagnostics: number;
}

export class LspDiagnosticRegistry {
  private readonly pending = new Map<string, PendingLspDiagnostic>();
  private readonly delivered = new Map<string, Set<string>>();
  private readonly deliveredOrder: string[] = [];

  register(serverName: string, files: LspDiagnosticFile[]): PendingLspDiagnostic {
    const entry: PendingLspDiagnostic = {
      id: randomUUID(),
      serverName,
      files,
      timestamp: Date.now(),
    };
    this.pending.set(entry.id, entry);
    return entry;
  }

  drainPending(): DrainedLspDiagnostics[] {
    if (this.pending.size === 0) {
      return [];
    }

    const batches = [...this.pending.values()];
    this.pending.clear();

    const grouped = new Map<string, LspDiagnosticFile[]>();
    for (const batch of batches) {
      const serverFiles = grouped.get(batch.serverName) ?? [];
      serverFiles.push(...batch.files);
      grouped.set(batch.serverName, serverFiles);
    }

    return [...grouped.entries()]
      .map(([serverName, files]) => {
        const deduped = this.deduplicateFiles(files);
        const totalDiagnostics = deduped.reduce(
          (sum, file) => sum + file.diagnostics.length,
          0,
        );
        if (totalDiagnostics === 0) {
          return null;
        }
        return {
          serverNames: [serverName],
          files: deduped,
          totalDiagnostics,
        } satisfies DrainedLspDiagnostics;
      })
      .filter((value): value is DrainedLspDiagnostics => value !== null);
  }

  clearDeliveredForUri(uri: string): void {
    this.delivered.delete(uri);
    const index = this.deliveredOrder.indexOf(uri);
    if (index >= 0) {
      this.deliveredOrder.splice(index, 1);
    }
  }

  clearAll(): void {
    this.pending.clear();
    this.delivered.clear();
    this.deliveredOrder.length = 0;
  }

  private deduplicateFiles(files: LspDiagnosticFile[]): LspDiagnosticFile[] {
    const seenByFile = new Map<string, Set<string>>();
    const results: LspDiagnosticFile[] = [];
    let totalDiagnostics = 0;

    for (const file of files) {
      const seenInBatch = seenByFile.get(file.uri) ?? new Set<string>();
      seenByFile.set(file.uri, seenInBatch);
      const delivered = this.delivered.get(file.uri) ?? new Set<string>();
      const output = results.find(candidate => candidate.uri === file.uri) ?? {
        uri: file.uri,
        diagnostics: [],
      };
      if (!results.some(candidate => candidate.uri === file.uri)) {
        results.push(output);
      }

      const sortedDiagnostics = [...file.diagnostics].sort((left, right) => {
        return severityRank(left.severity) - severityRank(right.severity);
      });

      for (const diagnostic of sortedDiagnostics) {
        if (
          output.diagnostics.length >= MAX_DIAGNOSTICS_PER_FILE ||
          totalDiagnostics >= MAX_TOTAL_DIAGNOSTICS
        ) {
          break;
        }
        const key = diagnosticKey(diagnostic);
        if (seenInBatch.has(key) || delivered.has(key)) {
          continue;
        }
        seenInBatch.add(key);
        delivered.add(key);
        output.diagnostics.push(diagnostic);
        totalDiagnostics += 1;
      }

      this.rememberDelivered(file.uri, delivered);
    }

    return results.filter(file => file.diagnostics.length > 0);
  }

  private rememberDelivered(uri: string, delivered: Set<string>): void {
    if (!this.delivered.has(uri)) {
      this.deliveredOrder.push(uri);
    }
    this.delivered.set(uri, delivered);

    while (this.deliveredOrder.length > MAX_TRACKED_FILES) {
      const evictedUri = this.deliveredOrder.shift();
      if (evictedUri) {
        this.delivered.delete(evictedUri);
      }
    }
  }
}

function diagnosticKey(diagnostic: LspDiagnosticEntry): string {
  return JSON.stringify({
    message: diagnostic.message,
    severity: diagnostic.severity,
    range: diagnostic.range,
    source: diagnostic.source ?? null,
    code: diagnostic.code ?? null,
  });
}

function severityRank(severity: LspDiagnosticSeverity): number {
  switch (severity) {
    case "Error":
      return 1;
    case "Warning":
      return 2;
    case "Info":
      return 3;
    case "Hint":
      return 4;
  }
}
