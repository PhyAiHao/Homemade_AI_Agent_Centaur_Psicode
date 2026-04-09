import { readFile } from "node:fs/promises";
import { extname, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import {
  loadLspConfigsFromEnv,
  normalizeFileExtension,
  normalizeLspServerConfig,
  type LspScopedServerConfig,
} from "./config.js";
import { LspDiagnosticRegistry, type DrainedLspDiagnostics } from "./diagnostic_registry.js";
import {
  registerLspNotificationHandlers,
  type LspDiagnosticEventWriter,
  type PassiveFeedbackRegistration,
} from "./passive_feedback.js";
import {
  createLspServerInstance,
  type LspServerInstance,
  type LspServerInstanceOptions,
} from "./server_instance.js";

interface OpenFileRecord {
  serverName: string;
  version: number;
}

export interface LspManagerOptions extends LspServerInstanceOptions {
  configs?: LspScopedServerConfig[];
  loadConfigs?: () => Promise<LspScopedServerConfig[]> | LspScopedServerConfig[];
  diagnosticRegistry?: LspDiagnosticRegistry;
  diagnosticWriter?: LspDiagnosticEventWriter;
}

export interface LspManager {
  initialize(): Promise<PassiveFeedbackRegistration>;
  shutdown(): Promise<void>;
  getAllServers(): Map<string, LspServerInstance>;
  getServerForFile(filePath: string): LspServerInstance | undefined;
  ensureServerStarted(filePath: string): Promise<LspServerInstance | undefined>;
  sendRequest<TResult>(
    filePath: string,
    method: string,
    params?: unknown,
  ): Promise<TResult | undefined>;
  openFile(filePath: string, content: string): Promise<void>;
  changeFile(filePath: string, content: string): Promise<void>;
  saveFile(filePath: string): Promise<void>;
  closeFile(filePath: string): Promise<void>;
  syncFileFromDisk(filePath: string): Promise<void>;
  isFileOpen(filePath: string): boolean;
  drainPendingDiagnostics(): DrainedLspDiagnostics[];
  clearDeliveredDiagnosticsForFile(filePath: string): void;
}

export function createLspManager(
  options: LspManagerOptions = {},
): LspManager {
  const servers = new Map<string, LspServerInstance>();
  const extensionMap = new Map<string, string[]>();
  const openedFiles = new Map<string, OpenFileRecord>();
  const registry = options.diagnosticRegistry ?? new LspDiagnosticRegistry();
  let initialized = false;
  let feedbackRegistration: PassiveFeedbackRegistration | undefined;

  async function initialize(): Promise<PassiveFeedbackRegistration> {
    if (initialized) {
      return (
        feedbackRegistration ?? {
          totalServers: servers.size,
          successCount: servers.size,
          registrationErrors: [],
        }
      );
    }

    const configs = await resolveConfigs(options);
    for (const config of configs) {
      const normalized = normalizeLspServerConfig(config);
      const instance = createLspServerInstance(normalized, options);
      servers.set(normalized.name, instance);
      for (const extension of Object.keys(normalized.extensionToLanguage)) {
        const normalizedExtension = normalizeFileExtension(extension);
        const owners = extensionMap.get(normalizedExtension) ?? [];
        owners.push(normalized.name);
        extensionMap.set(normalizedExtension, owners);
      }
      instance.onRequest("workspace/configuration", (params: unknown) => {
        const items =
          typeof params === "object" &&
          params !== null &&
          Array.isArray((params as { items?: unknown[] }).items)
            ? (params as { items: unknown[] }).items
            : [];
        return items.map(() => null);
      });
    }
    initialized = true;

    feedbackRegistration = registerLspNotificationHandlers(
      publicApi,
      registry,
      options.diagnosticWriter,
    );
    return feedbackRegistration;
  }

  async function shutdown(): Promise<void> {
    await Promise.allSettled([...servers.values()].map(server => server.stop()));
    servers.clear();
    extensionMap.clear();
    openedFiles.clear();
    registry.clearAll();
    feedbackRegistration = undefined;
    initialized = false;
  }

  function getAllServers(): Map<string, LspServerInstance> {
    return servers;
  }

  function getServerForFile(filePath: string): LspServerInstance | undefined {
    const extension = normalizeFileExtension(extname(filePath));
    const owners = extensionMap.get(extension);
    if (!owners || owners.length === 0) {
      return undefined;
    }
    return servers.get(owners[0]);
  }

  async function ensureServerStarted(
    filePath: string,
  ): Promise<LspServerInstance | undefined> {
    const server = getServerForFile(filePath);
    if (!server) {
      return undefined;
    }
    if (!server.isHealthy()) {
      await server.start();
    }
    return server;
  }

  async function sendRequest<TResult>(
    filePath: string,
    method: string,
    params?: unknown,
  ): Promise<TResult | undefined> {
    const server = await ensureServerStarted(filePath);
    if (!server) {
      return undefined;
    }
    return server.sendRequest<TResult>(method, params);
  }

  async function openFile(filePath: string, content: string): Promise<void> {
    const absolutePath = resolve(filePath);
    const server = await ensureServerStarted(absolutePath);
    if (!server) {
      return;
    }

    const fileUri = pathToFileURL(absolutePath).href;
    const existing = openedFiles.get(fileUri);
    if (existing?.serverName === server.name) {
      return;
    }

    const languageId =
      server.config.extensionToLanguage[
        normalizeFileExtension(extname(absolutePath))
      ] ?? "plaintext";

    await server.sendNotification("textDocument/didOpen", {
      textDocument: {
        uri: fileUri,
        languageId,
        version: 1,
        text: content,
      },
    });
    openedFiles.set(fileUri, {
      serverName: server.name,
      version: 1,
    });
  }

  async function changeFile(filePath: string, content: string): Promise<void> {
    const absolutePath = resolve(filePath);
    const fileUri = pathToFileURL(absolutePath).href;
    const record = openedFiles.get(fileUri);
    if (!record) {
      await openFile(absolutePath, content);
      return;
    }

    const server = servers.get(record.serverName) ?? getServerForFile(absolutePath);
    if (!server) {
      return;
    }
    if (!server.isHealthy()) {
      await server.start();
    }

    const nextVersion = record.version + 1;
    await server.sendNotification("textDocument/didChange", {
      textDocument: {
        uri: fileUri,
        version: nextVersion,
      },
      contentChanges: [
        {
          text: content,
        },
      ],
    });
    openedFiles.set(fileUri, {
      serverName: server.name,
      version: nextVersion,
    });
  }

  async function saveFile(filePath: string): Promise<void> {
    const absolutePath = resolve(filePath);
    const fileUri = pathToFileURL(absolutePath).href;
    const record = openedFiles.get(fileUri);
    if (!record) {
      return;
    }
    const server = servers.get(record.serverName);
    if (!server) {
      return;
    }
    await server.sendNotification("textDocument/didSave", {
      textDocument: {
        uri: fileUri,
      },
    });
  }

  async function closeFile(filePath: string): Promise<void> {
    const absolutePath = resolve(filePath);
    const fileUri = pathToFileURL(absolutePath).href;
    const record = openedFiles.get(fileUri);
    if (!record) {
      return;
    }
    const server = servers.get(record.serverName);
    if (server) {
      await server.sendNotification("textDocument/didClose", {
        textDocument: {
          uri: fileUri,
        },
      });
    }
    openedFiles.delete(fileUri);
  }

  async function syncFileFromDisk(filePath: string): Promise<void> {
    const absolutePath = resolve(filePath);
    const content = await readFile(absolutePath, "utf8");
    if (isFileOpen(absolutePath)) {
      await changeFile(absolutePath, content);
      return;
    }
    await openFile(absolutePath, content);
  }

  function isFileOpen(filePath: string): boolean {
    return openedFiles.has(pathToFileURL(resolve(filePath)).href);
  }

  function drainPendingDiagnostics(): DrainedLspDiagnostics[] {
    return registry.drainPending();
  }

  function clearDeliveredDiagnosticsForFile(filePath: string): void {
    registry.clearDeliveredForUri(resolve(filePath));
    registry.clearDeliveredForUri(pathToFileURL(resolve(filePath)).href);
  }

  const publicApi: LspManager = {
    initialize,
    shutdown,
    getAllServers,
    getServerForFile,
    ensureServerStarted,
    sendRequest,
    openFile,
    changeFile,
    saveFile,
    closeFile,
    syncFileFromDisk,
    isFileOpen,
    drainPendingDiagnostics,
    clearDeliveredDiagnosticsForFile,
  };

  return publicApi;
}

async function resolveConfigs(
  options: LspManagerOptions,
): Promise<LspScopedServerConfig[]> {
  if (options.configs) {
    return options.configs;
  }
  if (options.loadConfigs) {
    return await options.loadConfigs();
  }
  return loadLspConfigsFromEnv();
}
