import type { ApplicationEvent } from "./events.ts";
import type {
  ChatRequest,
  ChatStreamEvent,
  CommandError,
} from "./types.ts";

export interface TurnExecutionContext {
  turnId: string;
  assistantMessageId: string;
}

export type DesktopStreamEvent =
  | { type: "provider"; data: { event: ChatStreamEvent } }
  | { type: "error"; data: { error: CommandError } };

export interface StreamApplicationBatch {
  events: readonly ApplicationEvent[];
  terminal: boolean;
}

export function applicationEventsFromStream(
  message: DesktopStreamEvent,
  request: ChatRequest,
  context: TurnExecutionContext,
): StreamApplicationBatch {
  if (message.type === "error") {
    const summary =
      message.data.error.message ??
      "The selected provider could not complete this request.";
    return {
      terminal: true,
      events: [
        {
          type: "message_completed",
          data: {
            messageId: context.assistantMessageId,
            model: request.model,
            content: summary,
            finishReason: "error",
            usage: {},
          },
        },
        {
          type: "turn_receipt",
          data: {
            turnId: context.turnId,
            outcome: "failed",
            summary,
          },
        },
      ],
    };
  }

  const event = message.data.event;
  if (event.type === "message_delta") {
    return {
      terminal: false,
      events: [
        {
          type: "message_delta",
          data: {
            messageId: context.assistantMessageId,
            delta: event.data.delta,
          },
        },
      ],
    };
  }
  if (event.type === "message_completed") {
    const response = event.data.response;
    return {
      terminal: true,
      events: [
        {
          type: "message_completed",
          data: {
            messageId: context.assistantMessageId,
            model: response.model,
            content: response.content,
            finishReason: response.finishReason,
            usage: response.usage,
          },
        },
        {
          type: "turn_receipt",
          data: {
            turnId: context.turnId,
            outcome: "completed",
            summary: "Provider response completed.",
          },
        },
      ],
    };
  }
  return {
    terminal: true,
    events: [
      {
        type: "turn_receipt",
        data: {
          turnId: context.turnId,
          outcome: "cancelled",
          summary: event.data.reason,
        },
      },
    ],
  };
}
