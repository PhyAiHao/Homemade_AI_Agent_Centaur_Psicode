import type {
  BridgePermissionBroker,
  BridgePermissionRequest,
  BridgePermissionResponse,
} from "./types.js";

export function isBridgePermissionResponse(
  value: unknown,
): value is BridgePermissionResponse {
  return (
    !!value &&
    typeof value === "object" &&
    "behavior" in value &&
    ((value as { behavior?: unknown }).behavior === "allow" ||
      (value as { behavior?: unknown }).behavior === "deny")
  );
}

export class InMemoryBridgePermissionBroker implements BridgePermissionBroker {
  private readonly pending = new Map<string, BridgePermissionRequest>();
  private readonly requestListeners = new Set<
    (request: BridgePermissionRequest) => void
  >();
  private readonly responseListeners = new Map<
    string,
    Set<(response: BridgePermissionResponse) => void>
  >();
  private readonly waiters = new Map<
    string,
    {
      resolve(response: BridgePermissionResponse): void;
      reject(error: Error): void;
      timer?: ReturnType<typeof setTimeout>;
    }
  >();

  sendRequest(request: BridgePermissionRequest): void {
    this.pending.set(request.requestId, request);
    for (const listener of this.requestListeners) {
      listener(request);
    }
  }

  sendResponse(requestId: string, response: BridgePermissionResponse): void {
    this.pending.delete(requestId);
    const listeners = this.responseListeners.get(requestId);
    if (listeners) {
      for (const listener of listeners) {
        listener(response);
      }
    }
    const waiter = this.waiters.get(requestId);
    if (waiter) {
      if (waiter.timer) {
        clearTimeout(waiter.timer);
      }
      waiter.resolve(response);
      this.waiters.delete(requestId);
    }
  }

  cancelRequest(requestId: string): void {
    this.pending.delete(requestId);
    const waiter = this.waiters.get(requestId);
    if (waiter) {
      if (waiter.timer) {
        clearTimeout(waiter.timer);
      }
      waiter.reject(new Error(`Permission request ${requestId} was cancelled.`));
      this.waiters.delete(requestId);
    }
  }

  onResponse(
    requestId: string,
    handler: (response: BridgePermissionResponse) => void,
  ): () => void {
    let listeners = this.responseListeners.get(requestId);
    if (!listeners) {
      listeners = new Set();
      this.responseListeners.set(requestId, listeners);
    }
    listeners.add(handler);
    return () => {
      listeners?.delete(handler);
      if (listeners && listeners.size === 0) {
        this.responseListeners.delete(requestId);
      }
    };
  }

  onRequest(handler: (request: BridgePermissionRequest) => void): () => void {
    this.requestListeners.add(handler);
    return () => {
      this.requestListeners.delete(handler);
    };
  }

  waitForResponse(
    requestId: string,
    timeoutMs = 60_000,
  ): Promise<BridgePermissionResponse> {
    return new Promise((resolve, reject) => {
      const timer =
        timeoutMs > 0
          ? setTimeout(() => {
              this.waiters.delete(requestId);
              reject(
                new Error(
                  `Timed out waiting for permission response ${requestId}.`,
                ),
              );
            }, timeoutMs)
          : undefined;
      this.waiters.set(requestId, {
        resolve,
        reject,
        timer,
      });
    });
  }

  getPendingRequests(): BridgePermissionRequest[] {
    return [...this.pending.values()];
  }
}
