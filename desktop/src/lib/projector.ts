import type { ChatMessage } from "./types.ts";
import type {
  ProjectDefaults,
  ProjectSnapshot,
  RepositoryStatus,
} from "./types.ts";
import {
  APPLICATION_CONTRACT_VERSION,
  validateEvent,
  validateOrderedEvents,
  type ApplicationEvent,
  type ApprovalDecision,
  type ApprovalScope,
  type ArtifactKind,
  type EventEnvelope,
  type TaskStatus,
} from "./events.ts";

export interface UiMessage extends ChatMessage {
  id: string;
  label: string;
  time: string;
  model?: string;
  note?: string;
}

export interface ActivityItem {
  icon: string;
  title: string;
  detail: string;
  time: string;
  tone: "quiet" | "good" | "accent" | "warning";
}

export interface SessionProjection {
  sessionId: string;
  provider: string;
  model: string;
}

export interface ApprovalProjection {
  approvalId: string;
  action: string;
  resource: string;
  reason: string;
  decision?: ApprovalDecision;
  scope?: ApprovalScope;
}

export type ToolProjectionStatus =
  | "proposed"
  | "running"
  | "completed"
  | "failed";

export interface ToolProjection {
  toolCallId: string;
  name: string;
  summary: string;
  status: ToolProjectionStatus;
  outputChunks: number;
  exitCode?: number;
}

export interface ArtifactProjection {
  artifactId: string;
  kind: ArtifactKind;
  label: string;
}

export interface ReceiptProjection {
  turnId: string;
  outcome: "completed" | "cancelled" | "failed";
  summary: string;
}

export interface TaskProjection {
  schemaVersion: number;
  streamId?: string;
  taskId?: string;
  title?: string;
  messages: readonly UiMessage[];
  activity: readonly ActivityItem[];
  status: TaskStatus;
  running: boolean;
  session?: SessionProjection;
  activeTurnId?: string;
  pendingApproval?: ApprovalProjection;
  tools: readonly ToolProjection[];
  artifacts: readonly ArtifactProjection[];
  lastReceipt?: ReceiptProjection;
  lastSequence: number;
}

export interface InspectorProjection {
  pendingApproval?: ApprovalProjection;
  tools: readonly ToolProjection[];
  artifacts: readonly ArtifactProjection[];
  lastReceipt?: ReceiptProjection;
}

export interface WorkspaceProjection {
  workspaceId: string;
  projectId: string;
  path: string;
  isolated: boolean;
}

export interface ProjectEventProjection {
  schemaVersion: number;
  streamId?: string;
  project?: ProjectSnapshot;
  workspace?: WorkspaceProjection;
  lastSequence: number;
}

const emptyRepositoryStatus: RepositoryStatus = {
  staged: 0,
  modified: 0,
  untracked: 0,
  conflicts: 0,
  ahead: 0,
  behind: 0,
};

const emptyProjectDefaults: ProjectDefaults = {};

export function projectProjectEvents(
  events: readonly EventEnvelope[],
): ProjectEventProjection {
  validateOrderedEvents(events);
  return events.reduce<ProjectEventProjection>((current, event) => {
    validateEvent(event);
    if (event.sequence !== current.lastSequence + 1) {
      throw new Error(
        `Project projection expected sequence ${current.lastSequence + 1}, ` +
          `received ${event.sequence}.`,
      );
    }
    if (current.streamId && current.streamId !== event.streamId) {
      throw new Error(
        `Project projection belongs to stream ${current.streamId}; ` +
          `received ${event.streamId}.`,
      );
    }
    const identified = {
      ...current,
      streamId: current.streamId ?? event.streamId,
      lastSequence: event.sequence,
    };
    if (event.payload.type === "project_opened") {
      return {
        ...identified,
        project: {
          projectId: event.payload.data.projectId,
          displayName: event.payload.data.displayName,
          root: event.payload.data.root,
          branch: event.payload.data.branch,
          head: event.payload.data.head,
          status: event.payload.data.status ?? { ...emptyRepositoryStatus },
          defaults: event.payload.data.defaults ?? {
            ...emptyProjectDefaults,
          },
        },
      };
    }
    if (event.payload.type === "workspace_ready") {
      return {
        ...identified,
        workspace: {
          workspaceId: event.payload.data.workspaceId,
          projectId: event.payload.data.projectId,
          path: event.payload.data.path,
          isolated: event.payload.data.isolated,
        },
      };
    }
    throw new Error(
      "Project streams accept only project and workspace events.",
    );
  }, {
    schemaVersion: APPLICATION_CONTRACT_VERSION,
    lastSequence: 0,
  } satisfies ProjectEventProjection);
}

export function emptyTaskProjection(): TaskProjection {
  return {
    schemaVersion: APPLICATION_CONTRACT_VERSION,
    messages: [],
    activity: [],
    status: "queued",
    running: false,
    tools: [],
    artifacts: [],
    lastSequence: 0,
  };
}

export function projectEvents(
  events: readonly EventEnvelope[],
): TaskProjection {
  validateOrderedEvents(events);
  return events.reduce(projectEvent, emptyTaskProjection());
}

export function projectEvent(
  current: TaskProjection,
  event: EventEnvelope,
): TaskProjection {
  validateEvent(event);
  if (event.sequence !== current.lastSequence + 1) {
    throw new Error(
      `Projection expected sequence ${current.lastSequence + 1}, ` +
        `received ${event.sequence}.`,
    );
  }

  const identified: TaskProjection = {
    ...current,
    streamId: current.streamId ?? event.streamId,
    taskId: current.taskId ?? event.taskId,
  };
  if (identified.streamId !== event.streamId) {
    throw new Error(
      `Projection belongs to stream ${identified.streamId}; received ${event.streamId}.`,
    );
  }

  const next = applyPayload(identified, event.payload, event.occurredAtMs);
  return { ...next, lastSequence: event.sequence };
}

export function projectInspector(
  task: TaskProjection,
): InspectorProjection {
  return {
    pendingApproval: task.pendingApproval,
    tools: task.tools,
    artifacts: task.artifacts,
    lastReceipt: task.lastReceipt,
  };
}

function applyPayload(
  current: TaskProjection,
  payload: ApplicationEvent,
  occurredAtMs: number,
): TaskProjection {
  if (current.status === "cancelled" && isLateMutation(payload)) {
    return current;
  }

  const time = formatTime(occurredAtMs);

  switch (payload.type) {
    case "task_created":
      return addActivity(
        {
          ...current,
          title: payload.data.title,
          status: "queued",
          running: false,
        },
        {
          icon: "◇",
          title: "Task created",
          detail: payload.data.title,
          time,
          tone: "quiet",
        },
      );
    case "task_status_changed":
      return {
        ...current,
        status: payload.data.status,
        running: payload.data.status === "running",
      };
    case "session_started":
      return addActivity(
        {
          ...current,
          session: {
            sessionId: payload.data.sessionId,
            provider: payload.data.provider,
            model: payload.data.model,
          },
        },
        {
          icon: "◉",
          title: "Agent session started",
          detail: `${providerLabel(payload.data.provider)} · ${payload.data.model}`,
          time,
          tone: "quiet",
        },
      );
    case "turn_started":
      return addActivity(
        {
          ...current,
          activeTurnId: payload.data.turnId,
          running: true,
          status: "running",
        },
        {
          icon: "↳",
          title: "Turn started",
          detail: "Request accepted by the application core",
          time,
          tone: "quiet",
        },
      );
    case "message_added":
      return {
        ...current,
        messages: [
          ...current.messages,
          {
            id: payload.data.messageId,
            role: payload.data.role,
            label: payload.data.role === "user" ? "You" : "Kiln",
            time,
            content: payload.data.content,
          },
        ],
      };
    case "message_delta":
      return {
        ...current,
        messages: upsertAssistantMessage(
          current.messages,
          payload.data.messageId,
          payload.data.delta,
          time,
          true,
        ),
      };
    case "message_completed": {
      const note = payload.data.usage.totalTokens
        ? `${payload.data.usage.totalTokens.toLocaleString()} tokens`
        : "Completed";
      const messages = current.messages.filter(
        (message) => message.id !== payload.data.messageId,
      );
      return {
        ...current,
        messages: [
          ...messages,
          {
            id: payload.data.messageId,
            role: "assistant",
            label: "Kiln",
            time,
            model: payload.data.model,
            note,
            content: payload.data.content,
          },
        ],
      };
    }
    case "approval_requested":
      return addActivity(
        {
          ...current,
          status: "waiting_for_approval",
          running: false,
          pendingApproval: {
            approvalId: payload.data.approvalId,
            action: payload.data.action,
            resource: payload.data.resource,
            reason: payload.data.reason,
          },
        },
        {
          icon: "!",
          title: "Approval required",
          detail: `${payload.data.action} · ${payload.data.resource}`,
          time,
          tone: "warning",
        },
      );
    case "approval_decided":
      return addActivity(
        {
          ...current,
          status: "running",
          running: true,
          pendingApproval: undefined,
        },
        {
          icon: payload.data.decision === "approved" ? "✓" : "×",
          title:
            payload.data.decision === "approved"
              ? "Action approved"
              : "Action denied",
          detail: `${payload.data.scope} scope`,
          time,
          tone: payload.data.decision === "approved" ? "good" : "warning",
        },
      );
    case "tool_proposed":
      return addActivity(
        {
          ...current,
          tools: [
            ...current.tools,
            {
              toolCallId: payload.data.toolCallId,
              name: payload.data.name,
              summary: payload.data.summary,
              status: "proposed",
              outputChunks: 0,
            },
          ],
        },
        {
          icon: "◇",
          title: payload.data.name,
          detail: payload.data.summary,
          time,
          tone: "quiet",
        },
      );
    case "tool_started":
      return addActivity(
        {
          ...current,
          tools: patchTool(current.tools, payload.data.toolCallId, {
            status: "running",
          }),
        },
        {
          icon: "⌁",
          title: "Tool started",
          detail: payload.data.toolCallId,
          time,
          tone: "accent",
        },
      );
    case "tool_output":
      return addActivity(
        {
          ...current,
          tools: current.tools.map((tool) =>
            tool.toolCallId === payload.data.toolCallId
              ? { ...tool, outputChunks: tool.outputChunks + 1 }
              : tool,
          ),
        },
        {
          icon: "↳",
          title: "Tool result",
          detail: `${toolName(current, payload.data.toolCallId)} · ${payload.data.stream} output`,
          time,
          tone: payload.data.stream === "stderr" ? "warning" : "accent",
        },
      );
    case "tool_completed":
      return addActivity(
        {
          ...current,
          tools: patchTool(current.tools, payload.data.toolCallId, {
            status: payload.data.success ? "completed" : "failed",
            exitCode: payload.data.exitCode,
          }),
        },
        {
          icon: payload.data.success ? "✓" : "×",
          title: payload.data.success ? "Tool completed" : "Tool failed",
          detail:
            payload.data.exitCode === undefined
              ? payload.data.toolCallId
              : `${payload.data.toolCallId} · exit ${payload.data.exitCode}`,
          time,
          tone: payload.data.success ? "good" : "warning",
        },
      );
    case "artifact_published":
      return addActivity(
        {
          ...current,
          artifacts: [
            ...current.artifacts,
            {
              artifactId: payload.data.artifactId,
              kind: payload.data.kind,
              label: payload.data.label,
            },
          ],
        },
        {
          icon: "◇",
          title: artifactTitle(payload.data.kind),
          detail: payload.data.label,
          time,
          tone: "accent",
        },
      );
    case "turn_receipt": {
      const completed = payload.data.outcome === "completed";
      return addActivity(
        {
          ...current,
          activeTurnId: undefined,
          pendingApproval: undefined,
          running: false,
          status:
            payload.data.outcome === "completed"
              ? "completed"
              : payload.data.outcome,
          lastReceipt: {
            turnId: payload.data.turnId,
            outcome: payload.data.outcome,
            summary: payload.data.summary,
          },
        },
        {
          icon: completed ? "✓" : "×",
          title: completed ? "Ready for review" : "Turn ended",
          detail: payload.data.summary,
          time,
          tone: completed ? "good" : "warning",
        },
      );
    }
    case "project_opened":
    case "workspace_ready":
      return current;
  }
}

function isLateMutation(payload: ApplicationEvent): boolean {
  return (
    payload.type === "message_delta" ||
    payload.type === "message_completed" ||
    payload.type === "tool_started" ||
    payload.type === "tool_output" ||
    payload.type === "tool_completed"
  );
}

function patchTool(
  tools: readonly ToolProjection[],
  toolCallId: string,
  patch: Partial<ToolProjection>,
): readonly ToolProjection[] {
  return tools.map((tool) =>
    tool.toolCallId === toolCallId ? { ...tool, ...patch } : tool,
  );
}

function toolName(current: TaskProjection, toolCallId: string): string {
  return (
    current.tools.find((tool) => tool.toolCallId === toolCallId)?.name ??
    toolCallId
  );
}

function artifactTitle(kind: ArtifactKind): string {
  if (kind === "plan") return "Plan ready";
  if (kind === "diff") return "Diff ready";
  if (kind === "test_result") return "Tests recorded";
  if (kind === "command_output") return "Output captured";
  return "Artifact ready";
}

function addActivity(
  current: TaskProjection,
  item: ActivityItem,
): TaskProjection {
  return { ...current, activity: [...current.activity, item] };
}

function upsertAssistantMessage(
  messages: readonly UiMessage[],
  messageId: string,
  content: string,
  time: string,
  append: boolean,
): readonly UiMessage[] {
  const index = messages.findIndex((message) => message.id === messageId);
  if (index < 0) {
    return [
      ...messages,
      {
        id: messageId,
        role: "assistant",
        label: "Kiln",
        time,
        model: "streaming",
        content,
      },
    ];
  }

  return messages.map((message, messageIndex) =>
    messageIndex === index
      ? {
          ...message,
          content: append ? `${message.content}${content}` : content,
        }
      : message,
  );
}

function formatTime(occurredAtMs: number): string {
  return new Date(occurredAtMs).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
}

function providerLabel(provider: string): string {
  if (provider === "openai") return "OpenAI";
  if (provider === "anthropic") return "Anthropic";
  return "Local server";
}
