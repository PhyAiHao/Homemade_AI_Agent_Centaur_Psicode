class AsyncQueue<T> {
  private readonly items: T[] = [];
  private readonly waiters: Array<(result: IteratorResult<T>) => void> = [];
  private finished = false;

  push(item: T): void {
    const waiter = this.waiters.shift();
    if (waiter) {
      waiter({ value: item, done: false });
      return;
    }
    this.items.push(item);
  }

  end(): void {
    this.finished = true;
    while (this.waiters.length > 0) {
      const waiter = this.waiters.shift();
      waiter?.({ value: undefined as T, done: true });
    }
  }

  async next(): Promise<IteratorResult<T>> {
    if (this.items.length > 0) {
      const value = this.items.shift() as T;
      return { value, done: false };
    }
    if (this.finished) {
      return { value: undefined as T, done: true };
    }
    return new Promise(resolve => {
      this.waiters.push(resolve);
    });
  }
}

export interface WebSocketMessage {
  type: "message";
  data: string | ArrayBuffer | Blob;
}

export interface WebSocketFactory {
  create(url: string, protocols?: string | string[]): WebSocket;
}

class DefaultWebSocketFactory implements WebSocketFactory {
  create(url: string, protocols?: string | string[]): WebSocket {
    if (typeof WebSocket === "undefined") {
      throw new Error("WebSocket is not available in this runtime.");
    }
    return new WebSocket(url, protocols);
  }
}

export class WebSocketConnection implements AsyncIterable<WebSocketMessage> {
  private readonly queue = new AsyncQueue<WebSocketMessage>();
  private readonly ready: Promise<void>;

  constructor(readonly socket: WebSocket) {
    this.ready = new Promise((resolve, reject) => {
      const cleanup = (): void => {
        socket.removeEventListener("open", onOpen);
        socket.removeEventListener("error", onError);
      };
      const onOpen = (): void => {
        cleanup();
        resolve();
      };
      const onError = (): void => {
        cleanup();
        reject(new Error("WebSocket connection failed before opening."));
      };
      socket.addEventListener("open", onOpen, { once: true });
      socket.addEventListener("error", onError, { once: true });
    });

    socket.addEventListener("message", event => {
      this.queue.push({
        type: "message",
        data: event.data,
      });
    });
    socket.addEventListener("close", () => {
      this.queue.end();
    });
  }

  async waitUntilOpen(timeoutMs = 5_000): Promise<void> {
    await withTimeout(this.ready, timeoutMs, "Timed out waiting for WebSocket open.");
  }

  send(data: string | ArrayBufferLike | Blob | ArrayBufferView): void {
    this.socket.send(data);
  }

  sendJson(value: unknown): void {
    this.socket.send(JSON.stringify(value));
  }

  close(code?: number, reason?: string): void {
    this.socket.close(code, reason);
  }

  [Symbol.asyncIterator](): AsyncIterator<WebSocketMessage> {
    return {
      next: () => this.queue.next(),
    };
  }
}

export class WebSocketTransport {
  constructor(
    private readonly factory: WebSocketFactory = new DefaultWebSocketFactory(),
  ) {}

  async connect(
    url: string,
    protocols?: string | string[],
    timeoutMs?: number,
  ): Promise<WebSocketConnection> {
    const connection = new WebSocketConnection(this.factory.create(url, protocols));
    await connection.waitUntilOpen(timeoutMs);
    return connection;
  }
}

async function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  message: string,
): Promise<T> {
  let timeoutHandle: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<T>((_, reject) => {
        timeoutHandle = setTimeout(() => {
          reject(new Error(message));
        }, timeoutMs);
      }),
    ]);
  } finally {
    if (timeoutHandle !== undefined) {
      clearTimeout(timeoutHandle);
    }
  }
}
