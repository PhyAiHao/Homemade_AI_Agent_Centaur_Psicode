import { SseTransport, type SseEvent } from "./sse.js";
import {
  WebSocketConnection,
  WebSocketTransport,
  type WebSocketMessage,
} from "./websocket.js";

export type HybridTransportMode = "websocket" | "sse";

export type HybridTransportEvent =
  | ({ transport: "websocket" } & WebSocketMessage)
  | ({ transport: "sse" } & SseEvent);

export interface HybridConnectOptions {
  websocketUrl?: string;
  sseUrl?: string;
  protocols?: string | string[];
  requestInit?: RequestInit;
  signal?: AbortSignal;
}

export class HybridTransportSession
  implements AsyncIterable<HybridTransportEvent>
{
  constructor(
    readonly mode: HybridTransportMode,
    private readonly websocketConnection?: WebSocketConnection,
    private readonly sseEvents?: AsyncIterable<SseEvent>,
  ) {}

  sendJson(payload: unknown): void {
    if (!this.websocketConnection) {
      throw new Error("Hybrid transport is using SSE fallback and cannot send messages.");
    }
    this.websocketConnection.sendJson(payload);
  }

  close(code?: number, reason?: string): void {
    this.websocketConnection?.close(code, reason);
  }

  async *[Symbol.asyncIterator](): AsyncIterator<HybridTransportEvent> {
    if (this.websocketConnection) {
      for await (const event of this.websocketConnection) {
        yield {
          transport: "websocket",
          ...event,
        };
      }
      return;
    }

    if (!this.sseEvents) {
      return;
    }

    for await (const event of this.sseEvents) {
      yield {
        transport: "sse",
        ...event,
      };
    }
  }
}

export class HybridTransport {
  constructor(
    private readonly options: {
      websocket?: WebSocketTransport;
      sse?: SseTransport;
      websocketTimeoutMs?: number;
    } = {},
  ) {}

  async connect(options: HybridConnectOptions): Promise<HybridTransportSession> {
    const websocketTransport = this.options.websocket ?? new WebSocketTransport();
    const sseTransport = this.options.sse ?? new SseTransport();
    const websocketTimeoutMs = this.options.websocketTimeoutMs ?? 1_500;

    let websocketError: Error | undefined;
    if (options.websocketUrl) {
      try {
        const connection = await websocketTransport.connect(
          options.websocketUrl,
          options.protocols,
          websocketTimeoutMs,
        );
        return new HybridTransportSession("websocket", connection);
      } catch (error) {
        websocketError = error as Error;
      }
    }

    if (options.sseUrl) {
      return new HybridTransportSession(
        "sse",
        undefined,
        sseTransport.connect(options.sseUrl, {
          ...options.requestInit,
          signal: options.signal,
        }),
      );
    }

    throw websocketError ?? new Error("No usable transport endpoint was provided.");
  }
}
