import type {
  BridgeChatMessage,
  BridgeControlResponse,
  BridgeSessionHandle,
  SessionActivity,
  SessionDoneStatus,
  SessionRunner,
  SessionRunnerSpawnOptions,
} from "./types.js";

const MAX_ACTIVITIES = 20;

class LocalBridgeSessionHandle implements BridgeSessionHandle {
  readonly inboundMessages: BridgeChatMessage[] = [];
  readonly outboundMessages: BridgeChatMessage[] = [];
  readonly controlResponses: BridgeControlResponse[] = [];
  readonly activities: SessionActivity[] = [];
  currentActivity: SessionActivity | null = null;
  accessToken?: string;
  readonly done: Promise<SessionDoneStatus>;

  private doneStatus: SessionDoneStatus | null = null;
  private readonly resolveDone: (status: SessionDoneStatus) => void;

  constructor(
    readonly sessionId: string,
    accessToken: string | undefined,
    private readonly options: SessionRunnerSpawnOptions,
  ) {
    this.accessToken = accessToken;
    let resolveDone!: (status: SessionDoneStatus) => void;
    this.done = new Promise<SessionDoneStatus>(resolve => {
      resolveDone = resolve;
    });
    this.resolveDone = resolveDone;
  }

  async writeInbound(message: BridgeChatMessage): Promise<void> {
    this.inboundMessages.push(message);
    this.pushActivity("inbound", summarizeChatMessage(message));
    await this.options.onInboundMessage?.(message);
  }

  writeOutbound(message: BridgeChatMessage): void {
    this.outboundMessages.push(message);
    this.pushActivity("outbound", summarizeChatMessage(message));
  }

  writeControlResponse(response: BridgeControlResponse): void {
    this.controlResponses.push(response);
    this.pushActivity(
      "control",
      `control_response:${response.response.subtype}:${response.response.request_id}`,
    );
    this.options.onControlResponse?.(response);
  }

  complete(status: SessionDoneStatus, result = ""): void {
    if (this.doneStatus !== null) {
      return;
    }
    this.doneStatus = status;
    this.pushActivity(
      "result",
      result || (status === "completed" ? "session completed" : status),
    );
    this.resolveDone(status);
  }

  fail(error: string): void {
    if (this.doneStatus !== null) {
      return;
    }
    this.pushActivity("error", error);
    this.doneStatus = "failed";
    this.resolveDone("failed");
  }

  interrupt(): void {
    if (this.doneStatus !== null) {
      return;
    }
    this.pushActivity("control", "interrupt");
    this.doneStatus = "interrupted";
    this.resolveDone("interrupted");
  }

  updateAccessToken(token: string): void {
    this.accessToken = token;
  }

  private pushActivity(type: SessionActivity["type"], summary: string): void {
    const activity: SessionActivity = {
      type,
      summary,
      timestamp: Date.now(),
    };
    this.currentActivity = activity;
    this.activities.push(activity);
    if (this.activities.length > MAX_ACTIVITIES) {
      this.activities.splice(0, this.activities.length - MAX_ACTIVITIES);
    }
    this.options.onActivity?.(this.sessionId, activity);
  }
}

export class InMemorySessionRunner implements SessionRunner {
  spawn(options: SessionRunnerSpawnOptions): BridgeSessionHandle {
    return new LocalBridgeSessionHandle(
      options.sessionId,
      options.accessToken,
      options,
    );
  }
}

export function createSessionRunner(): SessionRunner {
  return new InMemorySessionRunner();
}

function summarizeChatMessage(message: BridgeChatMessage): string {
  const content = message.message.content;
  const text =
    typeof content === "string"
      ? content
      : content.find(block => block.type === "text")?.text ?? "";
  return `${message.type}:${text.replace(/\s+/g, " ").trim().slice(0, 120)}`;
}
