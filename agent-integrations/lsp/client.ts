import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { once } from "node:events";

const HEADER_SEPARATOR = "\r\n\r\n";
const CONTENT_LENGTH_PREFIX = "content-length:";
const DEFAULT_REQUEST_TIMEOUT_MS = 20_000;

export interface JsonRpcErrorShape {
  code: number;
  message: string;
  data?: unknown;
}

export class JsonRpcError extends Error {
  constructor(
    message: string,
    readonly code = -32000,
    readonly data?: unknown,
  ) {
    super(message);
    this.name = "JsonRpcError";
  }
}

export interface LspInitializeParams {
  processId: number;
  initializationOptions?: Record<string, unknown>;
  workspaceFolders?: Array<{
    uri: string;
    name: string;
  }>;
  rootPath?: string;
  rootUri?: string;
  capabilities?: Record<string, unknown>;
}

export interface LspInitializeResult {
  capabilities?: Record<string, unknown>;
  serverInfo?: Record<string, unknown>;
}

export interface LspClientStartOptions {
  env?: Record<string, string>;
  cwd?: string;
  requestTimeoutMs?: number;
}

export interface LspClient {
  readonly capabilities: Record<string, unknown> | undefined;
  readonly isInitialized: boolean;
  readonly pid: number | undefined;
  start(
    command: string,
    args?: string[],
    options?: LspClientStartOptions,
  ): Promise<void>;
  initialize(params: LspInitializeParams): Promise<LspInitializeResult>;
  sendRequest<TResult>(
    method: string,
    params?: unknown,
    timeoutMs?: number,
  ): Promise<TResult>;
  sendNotification(method: string, params?: unknown): Promise<void>;
  onNotification(method: string, handler: (params: unknown) => void): void;
  onRequest<TParams, TResult>(
    method: string,
    handler: (params: TParams) => TResult | Promise<TResult>,
  ): void;
  stop(timeoutMs?: number): Promise<void>;
}

type JsonRpcMessage =
  | {
      jsonrpc: "2.0";
      id: number;
      method: string;
      params?: unknown;
    }
  | {
      jsonrpc: "2.0";
      method: string;
      params?: unknown;
    }
  | {
      jsonrpc: "2.0";
      id: number;
      result?: unknown;
      error?: JsonRpcErrorShape;
    };

interface PendingRequest {
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
  timer?: NodeJS.Timeout;
}

export function createLspClient(
  serverName: string,
  onCrash?: (error: Error) => void,
): LspClient {
  let childProcess: ChildProcessWithoutNullStreams | undefined;
  let isInitialized = false;
  let capabilities: Record<string, unknown> | undefined;
  let nextId = 1;
  let isStopping = false;
  let readBuffer = Buffer.alloc(0);
  let requestTimeoutMs = DEFAULT_REQUEST_TIMEOUT_MS;
  const pendingRequests = new Map<number, PendingRequest>();
  const notificationHandlers = new Map<string, Set<(params: unknown) => void>>();
  const requestHandlers = new Map<
    string,
    (params: unknown) => unknown | Promise<unknown>
  >();

  async function start(
    command: string,
    args: string[] = [],
    options: LspClientStartOptions = {},
  ): Promise<void> {
    if (childProcess) {
      return;
    }

    requestTimeoutMs = options.requestTimeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS;
    isStopping = false;
    childProcess = spawn(command, args, {
      cwd: options.cwd,
      env: {
        ...processEnvForChild(),
        ...options.env,
      },
      stdio: "pipe",
      windowsHide: true,
    });

    const child = childProcess;
    child.stdout.on("data", handleStdoutChunk);
    child.stderr.on("data", chunk => {
      const text = chunk.toString("utf8").trim();
      if (text) {
        // Keep stderr attached for debugging without crashing the client.
        void text;
      }
    });
    child.on("exit", code => {
      const error =
        !isStopping && code !== 0
          ? new Error(`LSP server "${serverName}" exited with code ${String(code)}.`)
          : undefined;
      failAllPendingRequests(error ?? new Error(`LSP server "${serverName}" closed.`));
      childProcess = undefined;
      isInitialized = false;
      if (error) {
        onCrash?.(error);
      }
    });
    child.on("error", error => {
      failAllPendingRequests(error);
      childProcess = undefined;
      isInitialized = false;
      if (!isStopping) {
        onCrash?.(error);
      }
    });
    child.stdin.on("error", error => {
      if (!isStopping) {
        failAllPendingRequests(error);
      }
    });

    await Promise.race([
      once(child, "spawn").then(() => undefined),
      once(child, "error").then(([error]) => {
        throw error;
      }),
    ]);
  }

  async function initialize(
    params: LspInitializeParams,
  ): Promise<LspInitializeResult> {
    const result = await sendRequest<LspInitializeResult>("initialize", params);
    capabilities =
      typeof result.capabilities === "object" && result.capabilities !== null
        ? result.capabilities
        : undefined;
    isInitialized = true;
    await sendNotification("initialized", {});
    return result;
  }

  async function sendRequest<TResult>(
    method: string,
    params?: unknown,
    timeoutMs = requestTimeoutMs,
  ): Promise<TResult> {
    getStartedProcess();
    const id = nextId++;

    const promise = new Promise<TResult>((resolve, reject) => {
      const timer =
        timeoutMs > 0
          ? setTimeout(() => {
              pendingRequests.delete(id);
              reject(
                new Error(
                  `LSP request "${method}" timed out after ${timeoutMs}ms.`,
                ),
              );
            }, timeoutMs)
          : undefined;

      pendingRequests.set(id, {
        resolve: value => resolve(value as TResult),
        reject,
        timer,
      });
    });

    await writeMessage({
      jsonrpc: "2.0",
      id,
      method,
      ...(params === undefined ? {} : { params }),
    });

    return promise;
  }

  async function sendNotification(
    method: string,
    params?: unknown,
  ): Promise<void> {
    getStartedProcess();
    await writeMessage({
      jsonrpc: "2.0",
      method,
      ...(params === undefined ? {} : { params }),
    });
  }

  function onNotification(
    method: string,
    handler: (params: unknown) => void,
  ): void {
    const handlers = notificationHandlers.get(method) ?? new Set();
    handlers.add(handler);
    notificationHandlers.set(method, handlers);
  }

  function onRequest<TParams, TResult>(
    method: string,
    handler: (params: TParams) => TResult | Promise<TResult>,
  ): void {
    requestHandlers.set(
      method,
      handler as (params: unknown) => unknown | Promise<unknown>,
    );
  }

  async function stop(timeoutMs = 3_000): Promise<void> {
    if (!childProcess) {
      return;
    }

    const child = childProcess;
    isStopping = true;

    try {
      if (isInitialized) {
        try {
          await sendRequest("shutdown", undefined, timeoutMs);
        } catch {
          // Shutdown is best-effort here.
        }
        try {
          await sendNotification("exit");
        } catch {
          // Exit notification is best-effort too.
        }
      }
    } finally {
      child.stdin.end();
      const exitPromise = once(child, "exit").then(() => undefined).catch(() => undefined);
      const killTimer = setTimeout(() => {
        if (!child.killed) {
          child.kill("SIGTERM");
        }
      }, timeoutMs);
      await exitPromise;
      clearTimeout(killTimer);
      childProcess = undefined;
      isInitialized = false;
      capabilities = undefined;
      failAllPendingRequests(new Error(`LSP server "${serverName}" stopped.`));
      isStopping = false;
    }
  }

  function handleStdoutChunk(chunk: Buffer): void {
    readBuffer = Buffer.concat([readBuffer, chunk]);

    while (true) {
      const headerEnd = readBuffer.indexOf(HEADER_SEPARATOR);
      if (headerEnd < 0) {
        return;
      }

      const rawHeaders = readBuffer.slice(0, headerEnd).toString("utf8");
      const contentLength = parseContentLength(rawHeaders);
      if (contentLength === null) {
        throw new Error(`Invalid LSP frame from "${serverName}": missing Content-Length.`);
      }

      const messageStart = headerEnd + HEADER_SEPARATOR.length;
      if (readBuffer.length < messageStart + contentLength) {
        return;
      }

      const rawBody = readBuffer
        .slice(messageStart, messageStart + contentLength)
        .toString("utf8");
      readBuffer = readBuffer.slice(messageStart + contentLength);
      handleMessage(JSON.parse(rawBody) as JsonRpcMessage);
    }
  }

  function handleMessage(message: JsonRpcMessage): void {
    if ("id" in message && ("result" in message || "error" in message)) {
      const pending = pendingRequests.get(message.id);
      if (!pending) {
        return;
      }
      pendingRequests.delete(message.id);
      if (pending.timer) {
        clearTimeout(pending.timer);
      }
      if (message.error) {
        pending.reject(
          new JsonRpcError(
            message.error.message,
            message.error.code,
            message.error.data,
          ),
        );
        return;
      }
      pending.resolve(message.result);
      return;
    }

    if (isJsonRpcRequestMessage(message)) {
      void handleIncomingRequest(message);
      return;
    }

    if (!isJsonRpcNotificationMessage(message)) {
      return;
    }

    const handlers = notificationHandlers.get(message.method);
    if (!handlers) {
      return;
    }
    for (const handler of handlers) {
      handler(message.params);
    }
  }

  async function handleIncomingRequest(
    message: Extract<JsonRpcMessage, { id: number; method: string }>,
  ): Promise<void> {
    const handler = requestHandlers.get(message.method);
    if (!handler) {
      await writeMessage({
        jsonrpc: "2.0",
        id: message.id,
        error: {
          code: -32601,
          message: `Method "${message.method}" not implemented.`,
        },
      });
      return;
    }

    try {
      const result = await handler(message.params);
      await writeMessage({
        jsonrpc: "2.0",
        id: message.id,
        result,
      });
    } catch (error) {
      await writeMessage({
        jsonrpc: "2.0",
        id: message.id,
        error: {
          code: error instanceof JsonRpcError ? error.code : -32000,
          message: error instanceof Error ? error.message : String(error),
          ...(error instanceof JsonRpcError && error.data !== undefined
            ? { data: error.data }
            : {}),
        },
      });
    }
  }

  async function writeMessage(message: JsonRpcMessage): Promise<void> {
    getStartedProcess();
    const body = Buffer.from(JSON.stringify(message), "utf8");
    const header = Buffer.from(`Content-Length: ${body.byteLength}\r\n\r\n`, "utf8");
    await writeToStdin(Buffer.concat([header, body]));
  }

  async function writeToStdin(buffer: Buffer): Promise<void> {
    const process = getStartedProcess();
    await new Promise<void>((resolve, reject) => {
      process.stdin.write(buffer, error => {
        if (error) {
          reject(error);
          return;
        }
        resolve();
      });
    });
  }

  function getStartedProcess(): ChildProcessWithoutNullStreams {
    if (!childProcess) {
      throw new Error(`LSP server "${serverName}" is not started.`);
    }
    return childProcess;
  }

  function failAllPendingRequests(error: Error): void {
    for (const [id, pending] of pendingRequests) {
      if (pending.timer) {
        clearTimeout(pending.timer);
      }
      pending.reject(error);
      pendingRequests.delete(id);
    }
  }

  return {
    get capabilities(): Record<string, unknown> | undefined {
      return capabilities;
    },
    get isInitialized(): boolean {
      return isInitialized;
    },
    get pid(): number | undefined {
      return childProcess?.pid;
    },
    start,
    initialize,
    sendRequest,
    sendNotification,
    onNotification,
    onRequest,
    stop,
  };
}

function parseContentLength(headers: string): number | null {
  for (const line of headers.split("\r\n")) {
    if (line.toLowerCase().startsWith(CONTENT_LENGTH_PREFIX)) {
      const value = Number.parseInt(line.slice(CONTENT_LENGTH_PREFIX.length).trim(), 10);
      return Number.isFinite(value) ? value : null;
    }
  }
  return null;
}

function processEnvForChild(): Record<string, string> {
  return Object.fromEntries(
    Object.entries(process.env).filter(
      (entry): entry is [string, string] => typeof entry[1] === "string",
    ),
  );
}

function isJsonRpcRequestMessage(
  message: JsonRpcMessage,
): message is Extract<JsonRpcMessage, { id: number; method: string }> {
  return "id" in message && "method" in message;
}

function isJsonRpcNotificationMessage(
  message: JsonRpcMessage,
): message is Extract<JsonRpcMessage, { method: string }> {
  return "method" in message && !("id" in message);
}
