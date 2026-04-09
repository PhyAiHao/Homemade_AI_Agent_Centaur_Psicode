function formatDuration(ms: number): string {
  if (ms < 60_000) {
    return `${Math.round(ms / 1000)}s`;
  }
  const minutes = Math.floor(ms / 60_000);
  const seconds = Math.round((ms % 60_000) / 1000);
  return seconds > 0 ? `${minutes}m ${seconds}s` : `${minutes}m`;
}

export function decodeJwtPayload(token: string): unknown | null {
  const normalized = token.startsWith("sk-ant-si-")
    ? token.slice("sk-ant-si-".length)
    : token;
  const parts = normalized.split(".");
  if (parts.length !== 3 || !parts[1]) {
    return null;
  }
  try {
    const json = Buffer.from(parts[1], "base64url").toString("utf8");
    return JSON.parse(json) as unknown;
  } catch {
    return null;
  }
}

export function decodeJwtExpiry(token: string): number | null {
  const payload = decodeJwtPayload(token);
  if (
    payload &&
    typeof payload === "object" &&
    "exp" in payload &&
    typeof (payload as { exp?: unknown }).exp === "number"
  ) {
    return (payload as { exp: number }).exp;
  }
  return null;
}

const TOKEN_REFRESH_BUFFER_MS = 5 * 60 * 1000;
const FALLBACK_REFRESH_INTERVAL_MS = 30 * 60 * 1000;
const MAX_REFRESH_FAILURES = 3;
const REFRESH_RETRY_DELAY_MS = 60_000;

export function createTokenRefreshScheduler({
  getAccessToken,
  onRefresh,
  label,
  refreshBufferMs = TOKEN_REFRESH_BUFFER_MS,
}: {
  getAccessToken: () => string | undefined | Promise<string | undefined>;
  onRefresh: (sessionId: string, accessToken: string) => void | Promise<void>;
  label: string;
  refreshBufferMs?: number;
}): {
  schedule(sessionId: string, token: string): void;
  scheduleFromExpiresIn(sessionId: string, expiresInSeconds: number): void;
  cancel(sessionId: string): void;
  cancelAll(): void;
} {
  const timers = new Map<string, ReturnType<typeof setTimeout>>();
  const generations = new Map<string, number>();
  const failures = new Map<string, number>();

  function nextGeneration(sessionId: string): number {
    const next = (generations.get(sessionId) ?? 0) + 1;
    generations.set(sessionId, next);
    return next;
  }

  function clearTimer(sessionId: string): void {
    const timer = timers.get(sessionId);
    if (timer) {
      clearTimeout(timer);
      timers.delete(sessionId);
    }
  }

  function schedule(sessionId: string, token: string): void {
    const expiry = decodeJwtExpiry(token);
    if (!expiry) {
      return;
    }
    clearTimer(sessionId);
    const generation = nextGeneration(sessionId);
    const delayMs = expiry * 1000 - Date.now() - refreshBufferMs;
    if (delayMs <= 0) {
      void doRefresh(sessionId, generation);
      return;
    }
    timers.set(
      sessionId,
      setTimeout(() => {
        void doRefresh(sessionId, generation);
      }, delayMs),
    );
  }

  function scheduleFromExpiresIn(
    sessionId: string,
    expiresInSeconds: number,
  ): void {
    clearTimer(sessionId);
    const generation = nextGeneration(sessionId);
    const delayMs = Math.max(expiresInSeconds * 1000 - refreshBufferMs, 30_000);
    timers.set(
      sessionId,
      setTimeout(() => {
        void doRefresh(sessionId, generation);
      }, delayMs),
    );
  }

  async function doRefresh(
    sessionId: string,
    generation: number,
  ): Promise<void> {
    const currentGeneration = generations.get(sessionId);
    if (currentGeneration !== generation) {
      return;
    }

    let accessToken: string | undefined;
    try {
      accessToken = await getAccessToken();
    } catch {
      accessToken = undefined;
    }
    if (generations.get(sessionId) !== generation) {
      return;
    }

    if (!accessToken) {
      const count = (failures.get(sessionId) ?? 0) + 1;
      failures.set(sessionId, count);
      if (count < MAX_REFRESH_FAILURES) {
        timers.set(
          sessionId,
          setTimeout(() => {
            void doRefresh(sessionId, generation);
          }, REFRESH_RETRY_DELAY_MS),
        );
      }
      return;
    }

    failures.delete(sessionId);
    await onRefresh(sessionId, accessToken);
    timers.set(
      sessionId,
      setTimeout(() => {
        void doRefresh(sessionId, generation);
      }, FALLBACK_REFRESH_INTERVAL_MS),
    );
  }

  function cancel(sessionId: string): void {
    nextGeneration(sessionId);
    clearTimer(sessionId);
    failures.delete(sessionId);
  }

  function cancelAll(): void {
    for (const sessionId of generations.keys()) {
      nextGeneration(sessionId);
    }
    for (const timer of timers.values()) {
      clearTimeout(timer);
    }
    timers.clear();
    failures.clear();
  }

  return {
    schedule,
    scheduleFromExpiresIn,
    cancel,
    cancelAll,
  };
}

export { formatDuration };
