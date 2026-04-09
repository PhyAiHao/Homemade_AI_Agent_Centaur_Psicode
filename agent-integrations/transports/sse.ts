export interface SseEvent {
  event: string;
  data: string;
  id?: string;
  retry?: number;
  raw: string;
}

type FetchLike = typeof fetch;

export class SseTransport {
  constructor(
    private readonly fetchImpl?: FetchLike,
  ) {}

  async *connect(
    url: string,
    init: RequestInit = {},
  ): AsyncGenerator<SseEvent, void, undefined> {
    const response = await this.resolveFetchImpl()(url, {
      ...init,
      headers: {
        "accept": "text/event-stream",
        ...(init.headers ?? {}),
      },
    });
    if (!response.ok) {
      throw new Error(`SSE connection failed with ${response.status} ${response.statusText}`);
    }
    if (!response.body) {
      throw new Error("SSE connection did not provide a response body.");
    }

    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";

    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) {
          buffer += decoder.decode();
          break;
        }
        buffer += decoder.decode(value, { stream: true });
        buffer = buffer.replace(/\r\n/g, "\n");
        let separator = buffer.indexOf("\n\n");
        while (separator >= 0) {
          const block = buffer.slice(0, separator);
          buffer = buffer.slice(separator + 2);
          const parsed = parseSseBlock(block);
          if (parsed) {
            yield parsed;
          }
          separator = buffer.indexOf("\n\n");
        }
      }

      const trailing = parseSseBlock(buffer.trim());
      if (trailing) {
        yield trailing;
      }
    } finally {
      reader.releaseLock();
    }
  }

  private resolveFetchImpl(): FetchLike {
    if (this.fetchImpl) {
      return this.fetchImpl;
    }
    if (!globalThis.fetch) {
      throw new Error("global fetch is not available in this runtime");
    }
    return globalThis.fetch.bind(globalThis);
  }
}

export function parseSseBlock(block: string): SseEvent | null {
  if (!block.trim()) {
    return null;
  }

  let event = "message";
  let id: string | undefined;
  let retry: number | undefined;
  const dataLines: string[] = [];

  for (const line of block.split("\n")) {
    if (!line || line.startsWith(":")) {
      continue;
    }
    const separator = line.indexOf(":");
    const field = separator >= 0 ? line.slice(0, separator) : line;
    const rawValue = separator >= 0 ? line.slice(separator + 1) : "";
    const value = rawValue.startsWith(" ") ? rawValue.slice(1) : rawValue;

    switch (field) {
      case "event":
        event = value || "message";
        break;
      case "data":
        dataLines.push(value);
        break;
      case "id":
        id = value;
        break;
      case "retry":
        retry = Number.parseInt(value, 10);
        break;
      default:
        break;
    }
  }

  return {
    event,
    data: dataLines.join("\n"),
    id,
    retry: Number.isFinite(retry) ? retry : undefined,
    raw: block,
  };
}
