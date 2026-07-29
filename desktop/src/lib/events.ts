import type { ChatRole, ProviderId } from "./types.ts";
import type { ProjectDefaults, RepositoryStatus } from "./types.ts";

export const APPLICATION_CONTRACT_VERSION = 1;

export type TaskStatus =
  | "queued"
  | "running"
  | "waiting_for_approval"
  | "paused"
  | "completed"
  | "failed"
  | "cancelled";

export interface TokenUsage {
  inputTokens?: number;
  outputTokens?: number;
  totalTokens?: number;
}

export type ApprovalDecision = "approved" | "denied";
export type ApprovalScope = "once" | "session" | "project";
export type ArtifactKind =
  | "diff"
  | "file"
  | "plan"
  | "command_output"
  | "diagnostic"
  | "test_result";

export type ApplicationEvent =
  | {
      type: "project_opened";
      data: {
        projectId: string;
        root: string;
        displayName: string;
        branch?: string;
        head?: string;
        status?: RepositoryStatus;
        defaults?: ProjectDefaults;
      };
    }
  | {
      type: "workspace_ready";
      data: {
        workspaceId: string;
        projectId: string;
        path: string;
        isolated: boolean;
      };
    }
  | { type: "task_created"; data: { title: string } }
  | { type: "task_status_changed"; data: { status: TaskStatus } }
  | {
      type: "session_started";
      data: { sessionId: string; provider: ProviderId; model: string };
    }
  | { type: "turn_started"; data: { turnId: string } }
  | {
      type: "message_added";
      data: { messageId: string; role: ChatRole; content: string };
    }
  | {
      type: "message_delta";
      data: { messageId: string; delta: string };
    }
  | {
      type: "message_completed";
      data: {
        messageId: string;
        model: string;
        content: string;
        finishReason?: string;
        usage: TokenUsage;
      };
    }
  | {
      type: "approval_requested";
      data: {
        approvalId: string;
        action: string;
        resource: string;
        reason: string;
      };
    }
  | {
      type: "approval_decided";
      data: {
        approvalId: string;
        decision: ApprovalDecision;
        scope: ApprovalScope;
      };
    }
  | {
      type: "tool_proposed";
      data: { toolCallId: string; name: string; summary: string };
    }
  | { type: "tool_started"; data: { toolCallId: string } }
  | {
      type: "tool_output";
      data: {
        toolCallId: string;
        stream: "stdout" | "stderr" | "structured";
        chunk: string;
      };
    }
  | {
      type: "tool_completed";
      data: { toolCallId: string; success: boolean; exitCode?: number };
    }
  | {
      type: "artifact_published";
      data: {
        artifactId: string;
        kind: ArtifactKind;
        label: string;
      };
    }
  | {
      type: "turn_receipt";
      data: {
        turnId: string;
        outcome: "completed" | "cancelled" | "failed";
        summary: string;
      };
    };

export interface EventEnvelope {
  schemaVersion: number;
  eventId: string;
  streamId: string;
  taskId?: string;
  sequence: number;
  occurredAtMs: number;
  causationId?: string;
  correlationId?: string;
  payload: ApplicationEvent;
}

export interface EventMetadata {
  causationId?: string;
  correlationId?: string;
}

interface EventStreamOptions {
  taskId?: string;
  nextSequence?: number;
  clock?: () => number;
  eventId?: (sequence: number) => string;
}

export class ContractError extends Error {}

export class ApplicationEventStream {
  readonly streamId: string;
  readonly taskId?: string;

  #nextSequence: number;
  #clock: () => number;
  #eventId: (sequence: number) => string;

  constructor(streamId: string, options: EventStreamOptions = {}) {
    assertText("streamId", streamId);
    this.streamId = streamId;
    this.taskId = options.taskId;
    this.#nextSequence = options.nextSequence ?? 1;
    this.#clock = options.clock ?? Date.now;
    this.#eventId =
      options.eventId ??
      ((sequence) => `${streamId}:${sequence}:${globalThis.crypto.randomUUID()}`);
  }

  append(
    payload: ApplicationEvent,
    metadata: EventMetadata = {},
  ): EventEnvelope {
    const sequence = this.#nextSequence;
    const event: EventEnvelope = {
      schemaVersion: APPLICATION_CONTRACT_VERSION,
      eventId: this.#eventId(sequence),
      streamId: this.streamId,
      taskId: this.taskId,
      sequence,
      occurredAtMs: this.#clock(),
      causationId: metadata.causationId,
      correlationId: metadata.correlationId,
      payload,
    };

    validateEvent(event);
    this.#nextSequence += 1;
    return event;
  }

  get nextSequence(): number {
    return this.#nextSequence;
  }
}

export function validateEvent(event: EventEnvelope): void {
  if (event.schemaVersion !== APPLICATION_CONTRACT_VERSION) {
    throw new ContractError(
      `Unsupported application contract version ${event.schemaVersion}; ` +
        `this build supports ${APPLICATION_CONTRACT_VERSION}.`,
    );
  }
  assertText("eventId", event.eventId);
  assertText("streamId", event.streamId);
  if (!Number.isSafeInteger(event.sequence) || event.sequence < 1) {
    throw new ContractError("sequence must be a positive safe integer.");
  }
}

export function validateOrderedEvents(events: readonly EventEnvelope[]): void {
  if (events.length === 0) return;

  const streamId = events[0].streamId;
  let expected = events[0].sequence;
  for (const event of events) {
    validateEvent(event);
    if (event.streamId !== streamId) {
      throw new ContractError(
        `Event belongs to stream ${event.streamId}; expected ${streamId}.`,
      );
    }
    if (event.sequence !== expected) {
      throw new ContractError(
        `Event sequence is ${event.sequence}; expected ${expected}.`,
      );
    }
    expected += 1;
  }
}

function assertText(field: string, value: string): void {
  if (!value.trim()) {
    throw new ContractError(`${field} cannot be blank.`);
  }
}
