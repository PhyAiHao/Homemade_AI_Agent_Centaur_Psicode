import type {
  ToolInvocationContext,
  ToolResultMessage,
  ToolResultWriter,
} from "./contracts.js";

export function createToolResultMessage(
  context: ToolInvocationContext,
  output: unknown,
  isError = false,
): ToolResultMessage {
  return {
    type: "tool_result",
    request_id: context.requestId,
    tool_call_id: context.toolCallId,
    output,
    ...(isError ? { is_error: true } : {}),
  };
}

export class ToolResultForwarder {
  constructor(private readonly writer: ToolResultWriter) {}

  async forward(
    context: ToolInvocationContext,
    output: unknown,
    isError = false,
  ): Promise<ToolResultMessage> {
    const message = createToolResultMessage(context, output, isError);
    await this.writer.write(message);
    return message;
  }
}

export class MemoryToolResultWriter implements ToolResultWriter {
  readonly messages: ToolResultMessage[] = [];

  write(message: ToolResultMessage): void {
    this.messages.push(message);
  }
}
