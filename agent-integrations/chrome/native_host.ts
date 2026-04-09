import { chmod, mkdir, unlink } from "node:fs/promises";
import { createServer, type Server, type Socket } from "node:net";

import { getSecureSocketPath, getSocketDir } from "./common.js";

const VERSION = "1.0.0";
const MAX_MESSAGE_SIZE = 1024 * 1024;

type HostLogger = (message: string, ...args: unknown[]) => void;

type McpClient = {
  id: number;
  socket: Socket;
  buffer: Buffer;
};

export interface ChromeNativeHostOptions {
  logger?: HostLogger;
  socketPath?: string;
  stdin?: NodeJS.ReadableStream;
  stdout?: NodeJS.WritableStream;
}

function defaultLogger(message: string, ...args: unknown[]): void {
  console.error(`[Centaur Chrome Native Host] ${message}`, ...args);
}

export function sendChromeMessage(
  payload: string | Record<string, unknown>,
  stdout: NodeJS.WritableStream = process.stdout,
): void {
  const message =
    typeof payload === "string" ? payload : JSON.stringify(payload);
  const bytes = Buffer.from(message, "utf8");
  const length = Buffer.alloc(4);
  length.writeUInt32LE(bytes.length, 0);
  stdout.write(length);
  stdout.write(bytes);
}

export class ChromeNativeHost {
  private readonly logger: HostLogger;
  private readonly stdin: NodeJS.ReadableStream;
  private readonly stdout: NodeJS.WritableStream;
  private readonly configuredSocketPath?: string;
  private readonly clients = new Map<number, McpClient>();
  private nextClientId = 1;
  private server: Server | null = null;
  private socketPath: string | null = null;
  private running = false;

  constructor(options: ChromeNativeHostOptions = {}) {
    this.logger = options.logger ?? defaultLogger;
    this.stdin = options.stdin ?? process.stdin;
    this.stdout = options.stdout ?? process.stdout;
    this.configuredSocketPath = options.socketPath;
  }

  async start(): Promise<void> {
    if (this.running) {
      return;
    }

    this.socketPath = this.configuredSocketPath ?? getSecureSocketPath();

    if (process.platform !== "win32") {
      await mkdir(getSocketDir(), { recursive: true, mode: 0o700 });
      await unlink(this.socketPath).catch(() => undefined);
    }

    this.server = createServer(socket => this.handleMcpClient(socket));
    await new Promise<void>((resolve, reject) => {
      this.server?.once("error", reject);
      this.server?.listen(this.socketPath!, () => resolve());
    });

    if (process.platform !== "win32") {
      await chmod(this.socketPath, 0o600).catch(() => undefined);
    }

    this.running = true;
  }

  async stop(): Promise<void> {
    if (!this.running) {
      return;
    }

    for (const client of this.clients.values()) {
      client.socket.destroy();
    }
    this.clients.clear();

    if (this.server) {
      await new Promise<void>(resolve => {
        this.server?.close(() => resolve());
      });
      this.server = null;
    }

    if (this.socketPath && process.platform !== "win32") {
      await unlink(this.socketPath).catch(() => undefined);
    }

    this.running = false;
  }

  isRunning(): boolean {
    return this.running;
  }

  getClientCount(): number {
    return this.clients.size;
  }

  async runUntilStdinClose(): Promise<void> {
    const reader = new ChromeMessageReader(this.stdin, this.logger);
    await this.start();
    try {
      while (true) {
        const message = await reader.read();
        if (message === null) {
          break;
        }
        await this.handleChromePayload(message);
      }
    } finally {
      await this.stop();
    }
  }

  async handleChromePayload(messageJson: string): Promise<void> {
    let rawMessage: unknown;
    try {
      rawMessage = JSON.parse(messageJson);
    } catch {
      this.sendError("Invalid message format");
      return;
    }

    if (
      typeof rawMessage !== "object" ||
      rawMessage === null ||
      typeof (rawMessage as { type?: unknown }).type !== "string"
    ) {
      this.sendError("Invalid message format");
      return;
    }

    const message = rawMessage as { type: string } & Record<string, unknown>;
    switch (message.type) {
      case "ping":
        sendChromeMessage(
          { type: "pong", timestamp: Date.now() },
          this.stdout,
        );
        return;
      case "get_status":
        sendChromeMessage(
          {
            type: "status_response",
            native_host_version: VERSION,
          },
          this.stdout,
        );
        return;
      case "tool_response":
      case "notification": {
        const { type: _ignored, ...forwarded } = message;
        this.forwardToMcpClients(forwarded);
        return;
      }
      default:
        this.sendError(`Unknown message type: ${message.type}`);
    }
  }

  private sendError(error: string): void {
    sendChromeMessage({ type: "error", error }, this.stdout);
  }

  private forwardToMcpClients(payload: Record<string, unknown>): void {
    const bytes = Buffer.from(JSON.stringify(payload), "utf8");
    const length = Buffer.alloc(4);
    length.writeUInt32LE(bytes.length, 0);
    const framed = Buffer.concat([length, bytes]);

    for (const client of this.clients.values()) {
      try {
        client.socket.write(framed);
      } catch (error) {
        this.logger("Failed to forward browser payload", error);
      }
    }
  }

  private handleMcpClient(socket: Socket): void {
    const clientId = this.nextClientId++;
    const client: McpClient = {
      id: clientId,
      socket,
      buffer: Buffer.alloc(0),
    };

    this.clients.set(clientId, client);
    sendChromeMessage({ type: "mcp_connected" }, this.stdout);

    socket.on("data", data => {
      client.buffer = Buffer.concat([client.buffer, data]);
      this.drainClientBuffer(client);
    });

    socket.on("close", () => {
      this.clients.delete(clientId);
      sendChromeMessage({ type: "mcp_disconnected" }, this.stdout);
    });

    socket.on("error", error => {
      this.logger(`MCP client ${clientId} error`, error);
    });
  }

  private drainClientBuffer(client: McpClient): void {
    while (client.buffer.length >= 4) {
      const length = client.buffer.readUInt32LE(0);
      if (length === 0 || length > MAX_MESSAGE_SIZE) {
        client.socket.destroy();
        return;
      }
      if (client.buffer.length < 4 + length) {
        return;
      }

      const payload = client.buffer.subarray(4, 4 + length).toString("utf8");
      client.buffer = client.buffer.subarray(4 + length);

      try {
        const request = JSON.parse(payload) as {
          method?: string;
          params?: unknown;
        };
        if (typeof request.method !== "string") {
          continue;
        }
        sendChromeMessage(
          {
            type: "tool_request",
            method: request.method,
            params: request.params,
          },
          this.stdout,
        );
      } catch (error) {
        this.logger(`Failed to parse MCP payload from client ${client.id}`, error);
      }
    }
  }
}

class ChromeMessageReader {
  private buffer = Buffer.alloc(0);
  private pendingResolve: ((value: string | null) => void) | null = null;
  private closed = false;

  constructor(
    stdin: NodeJS.ReadableStream,
    private readonly logger: HostLogger,
  ) {
    stdin.on("data", chunk => {
      const incoming = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
      this.buffer = Buffer.concat([this.buffer, incoming]);
      this.tryResolve();
    });
    stdin.on("end", () => {
      this.closed = true;
      this.tryResolveClosed();
    });
    stdin.on("error", () => {
      this.closed = true;
      this.tryResolveClosed();
    });
  }

  async read(): Promise<string | null> {
    if (this.closed && this.buffer.length === 0) {
      return null;
    }

    const buffered = this.tryReadBuffered();
    if (buffered !== undefined) {
      return buffered;
    }

    return await new Promise(resolve => {
      this.pendingResolve = resolve;
      this.tryResolve();
    });
  }

  private tryResolve(): void {
    if (!this.pendingResolve) {
      return;
    }
    const value = this.tryReadBuffered();
    if (value === undefined) {
      return;
    }
    this.pendingResolve(value);
    this.pendingResolve = null;
  }

  private tryResolveClosed(): void {
    if (!this.pendingResolve) {
      return;
    }
    this.pendingResolve(null);
    this.pendingResolve = null;
  }

  private tryReadBuffered(): string | null | undefined {
    if (this.buffer.length < 4) {
      return this.closed ? null : undefined;
    }

    const length = this.buffer.readUInt32LE(0);
    if (length === 0 || length > MAX_MESSAGE_SIZE) {
      this.logger("Invalid Chrome message length", length);
      return null;
    }
    if (this.buffer.length < 4 + length) {
      return this.closed ? null : undefined;
    }

    const payload = this.buffer.subarray(4, 4 + length).toString("utf8");
    this.buffer = this.buffer.subarray(4 + length);
    return payload;
  }
}

export async function runChromeNativeHost(
  options: ChromeNativeHostOptions = {},
): Promise<void> {
  const host = new ChromeNativeHost(options);
  await host.runUntilStdinClose();
}
