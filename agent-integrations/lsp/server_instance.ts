import { basename } from "node:path";
import { pathToFileURL } from "node:url";
import { createLspClient, JsonRpcError, type LspClient } from "./client.js";
import type { LspScopedServerConfig, LspServerState } from "./config.js";

const CONTENT_MODIFIED_ERROR_CODE = -32801;
const DEFAULT_STARTUP_TIMEOUT_MS = 10_000;
const DEFAULT_SHUTDOWN_TIMEOUT_MS = 3_000;
const DEFAULT_TRANSIENT_RETRY_COUNT = 3;
const DEFAULT_TRANSIENT_RETRY_BASE_DELAY_MS = 500;

export interface LspServerInstance {
  readonly name: string;
  readonly config: LspScopedServerConfig;
  readonly state: LspServerState;
  readonly startTime: Date | undefined;
  readonly lastError: Error | undefined;
  readonly restartCount: number;
  readonly clientPid: number | undefined;
  start(): Promise<void>;
  stop(): Promise<void>;
  restart(): Promise<void>;
  isHealthy(): boolean;
  sendRequest<TResult>(method: string, params?: unknown): Promise<TResult>;
  sendNotification(method: string, params?: unknown): Promise<void>;
  onNotification(method: string, handler: (params: unknown) => void): void;
  onRequest<TParams, TResult>(
    method: string,
    handler: (params: TParams) => TResult | Promise<TResult>,
  ): void;
}

export interface LspServerInstanceOptions {
  clientFactory?: (
    serverName: string,
    onCrash?: (error: Error) => void,
  ) => LspClient;
}

export function createLspServerInstance(
  config: LspScopedServerConfig,
  options: LspServerInstanceOptions = {},
): LspServerInstance {
  let state: LspServerState = "stopped";
  let startTime: Date | undefined;
  let lastError: Error | undefined;
  let restartCount = 0;
  let crashCount = 0;
  const createClient = options.clientFactory ?? createLspClient;
  const client = createClient(config.name, error => {
    state = "error";
    lastError = error;
    crashCount += 1;
  });

  async function start(): Promise<void> {
    if (state === "running" || state === "starting") {
      return;
    }

    const maxRestartCount = config.maxRestartCount ?? DEFAULT_TRANSIENT_RETRY_COUNT;
    if (state === "error" && crashCount > maxRestartCount) {
      throw new Error(
        `LSP server "${config.name}" exceeded crash recovery limit (${maxRestartCount}).`,
      );
    }

    state = "starting";
    try {
      await client.start(config.command, config.args, {
        ...(config.env ? { env: config.env } : {}),
        ...(config.workspaceFolder ? { cwd: config.workspaceFolder } : {}),
        ...(config.requestTimeoutMs ? { requestTimeoutMs: config.requestTimeoutMs } : {}),
      });

      const workspaceFolder = config.workspaceFolder ?? process.cwd();
      const workspaceUri = pathToFileURL(workspaceFolder).href;
      await withTimeout(
        client.initialize({
          processId: process.pid,
          ...(config.initializationOptions
            ? { initializationOptions: config.initializationOptions }
            : {}),
          workspaceFolders: [
            {
              uri: workspaceUri,
              name: basename(workspaceFolder),
            },
          ],
          rootPath: workspaceFolder,
          rootUri: workspaceUri,
          capabilities: defaultClientCapabilities(),
        }),
        config.startupTimeoutMs ?? DEFAULT_STARTUP_TIMEOUT_MS,
        `LSP server "${config.name}" timed out during initialization.`,
      );

      state = "running";
      startTime = new Date();
      lastError = undefined;
      crashCount = 0;
    } catch (error) {
      state = "error";
      lastError = error as Error;
      try {
        await client.stop(config.shutdownTimeoutMs ?? DEFAULT_SHUTDOWN_TIMEOUT_MS);
      } catch {
        // Cleanup is best-effort on failed start.
      }
      throw error;
    }
  }

  async function stop(): Promise<void> {
    if (state === "stopped") {
      return;
    }
    state = "stopping";
    try {
      await client.stop(config.shutdownTimeoutMs ?? DEFAULT_SHUTDOWN_TIMEOUT_MS);
      state = "stopped";
      startTime = undefined;
    } catch (error) {
      state = "error";
      lastError = error as Error;
      throw error;
    }
  }

  async function restart(): Promise<void> {
    restartCount += 1;
    await stop().catch(() => undefined);
    await start();
  }

  function isHealthy(): boolean {
    return state === "running" && client.isInitialized;
  }

  async function sendRequest<TResult>(
    method: string,
    params?: unknown,
  ): Promise<TResult> {
    if (state !== "running") {
      throw new Error(`LSP server "${config.name}" is not running.`);
    }

    const retryCount =
      config.transientRetryCount ?? DEFAULT_TRANSIENT_RETRY_COUNT;
    const retryBaseDelayMs =
      config.transientRetryBaseDelayMs ?? DEFAULT_TRANSIENT_RETRY_BASE_DELAY_MS;

    let attempt = 0;
    while (true) {
      try {
        return await client.sendRequest<TResult>(
          method,
          params,
          config.requestTimeoutMs,
        );
      } catch (error) {
        if (
          error instanceof JsonRpcError &&
          error.code === CONTENT_MODIFIED_ERROR_CODE &&
          attempt < retryCount
        ) {
          await sleep(retryBaseDelayMs * 2 ** attempt);
          attempt += 1;
          continue;
        }
        lastError = error as Error;
        throw error;
      }
    }
  }

  async function sendNotification(
    method: string,
    params?: unknown,
  ): Promise<void> {
    if (state !== "running") {
      throw new Error(`LSP server "${config.name}" is not running.`);
    }
    await client.sendNotification(method, params);
  }

  function onNotification(
    method: string,
    handler: (params: unknown) => void,
  ): void {
    client.onNotification(method, handler);
  }

  function onRequest<TParams, TResult>(
    method: string,
    handler: (params: TParams) => TResult | Promise<TResult>,
  ): void {
    client.onRequest(method, handler);
  }

  return {
    get name(): string {
      return config.name;
    },
    config,
    get state(): LspServerState {
      return state;
    },
    get startTime(): Date | undefined {
      return startTime;
    },
    get lastError(): Error | undefined {
      return lastError;
    },
    get restartCount(): number {
      return restartCount;
    },
    get clientPid(): number | undefined {
      return client.pid;
    },
    start,
    stop,
    restart,
    isHealthy,
    sendRequest,
    sendNotification,
    onNotification,
    onRequest,
  };
}

async function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  message: string,
): Promise<T> {
  let timer: NodeJS.Timeout | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<T>((_, reject) => {
        timer = setTimeout(() => reject(new Error(message)), timeoutMs);
      }),
    ]);
  } finally {
    if (timer) {
      clearTimeout(timer);
    }
  }
}

async function sleep(ms: number): Promise<void> {
  await new Promise(resolve => setTimeout(resolve, ms));
}

function defaultClientCapabilities(): Record<string, unknown> {
  return {
    workspace: {
      configuration: false,
      workspaceFolders: false,
    },
    textDocument: {
      synchronization: {
        dynamicRegistration: false,
        willSave: false,
        willSaveWaitUntil: false,
        didSave: true,
      },
      publishDiagnostics: {
        relatedInformation: true,
        codeDescriptionSupport: true,
      },
      hover: {
        dynamicRegistration: false,
        contentFormat: ["markdown", "plaintext"],
      },
      definition: {
        dynamicRegistration: false,
        linkSupport: true,
      },
      references: {
        dynamicRegistration: false,
      },
      documentSymbol: {
        dynamicRegistration: false,
        hierarchicalDocumentSymbolSupport: true,
      },
      callHierarchy: {
        dynamicRegistration: false,
      },
    },
    general: {
      positionEncodings: ["utf-16"],
    },
  };
}
