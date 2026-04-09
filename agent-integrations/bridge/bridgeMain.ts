import type {
  BridgeMainLoopConfig,
  BridgeStatusSnapshot,
  ConnectedRemoteBridge,
  RemoteBridgeConnectOptions,
} from "./types.js";
import { RemoteBridge } from "./remoteBridge.js";

export class BridgeMainLoop {
  private readonly maxSessions: number;
  private readonly logger: Required<NonNullable<BridgeMainLoopConfig["logger"]>>;
  private readonly sessions = new Map<string, ConnectedRemoteBridge>();

  constructor(
    private readonly remoteBridge: RemoteBridge,
    config: BridgeMainLoopConfig = {},
  ) {
    this.maxSessions = config.maxSessions ?? 8;
    this.logger = config.logger ?? console;
  }

  async startSession(
    options: RemoteBridgeConnectOptions,
  ): Promise<ConnectedRemoteBridge> {
    if (this.sessions.size >= this.maxSessions) {
      throw new Error(
        `Bridge is at capacity (${this.sessions.size}/${this.maxSessions} sessions).`,
      );
    }
    const connection = await this.remoteBridge.connect(options);
    this.sessions.set(connection.sessionId, connection);
    this.logger.info(
      `Bridge session started: ${connection.sessionId} (${connection.title})`,
    );
    return {
      ...connection,
      close: async () => {
        await connection.close();
        this.sessions.delete(connection.sessionId);
        this.logger.info(`Bridge session closed: ${connection.sessionId}`);
      },
    };
  }

  async stopSession(sessionId: string): Promise<void> {
    const connection = this.sessions.get(sessionId);
    if (!connection) {
      return;
    }
    await connection.close();
    this.sessions.delete(sessionId);
    this.logger.info(`Bridge session stopped: ${sessionId}`);
  }

  getStatus(): BridgeStatusSnapshot {
    return {
      activeSessions: this.sessions.size,
      maxSessions: this.maxSessions,
      sessions: [...this.sessions.values()].map(connection => ({
        sessionId: connection.sessionId,
        title: connection.title,
        state: connection.bridge.getState(),
      })),
    };
  }

  async shutdown(): Promise<void> {
    for (const sessionId of [...this.sessions.keys()]) {
      await this.stopSession(sessionId);
    }
  }
}

export async function runBridgeMainLoop(options: {
  mainLoop: BridgeMainLoop;
  initialSession: RemoteBridgeConnectOptions;
  signal?: AbortSignal;
}): Promise<ConnectedRemoteBridge> {
  const connection = await options.mainLoop.startSession(options.initialSession);
  if (options.signal) {
    options.signal.addEventListener(
      "abort",
      () => {
        void options.mainLoop.shutdown();
      },
      { once: true },
    );
  }
  return connection;
}
