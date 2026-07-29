import assert from "node:assert/strict";
import test from "node:test";

import {
  APPLICATION_CONTRACT_VERSION,
  ApplicationEventStream,
  type ApplicationEvent,
  validateEvent,
  validateOrderedEvents,
} from "../desktop/src/lib/events.ts";
import { executeVisibleRepositoryTool } from "../desktop/src/lib/bridge.ts";
import { DurableTaskHistory } from "../desktop/src/lib/history.ts";
import { initialSessionEvents } from "../desktop/src/lib/preview-session.ts";
import {
  projectEvents,
  projectInspector,
  projectProjectEvents,
} from "../desktop/src/lib/projector.ts";
import { applicationEventsFromStream } from "../desktop/src/lib/stream-events.ts";

test("replays the complete recorded session deterministically", () => {
  const first = projectEvents(initialSessionEvents);
  const second = projectEvents(initialSessionEvents);
  const inspector = projectInspector(first);

  assert.deepEqual(first, second);
  assert.equal(JSON.stringify(first), JSON.stringify(second));
  assert.equal(initialSessionEvents.length, 25);
  assert.equal(first.title, "Polish the provider status card");
  assert.equal(first.messages.length, 2);
  assert.equal(first.messages[0].role, "user");
  assert.equal(first.messages[1].role, "assistant");
  assert.equal(first.messages[1].model, "qwen3-coder");
  assert.equal(first.status, "completed");
  assert.equal(first.running, false);
  assert.equal(first.lastSequence, initialSessionEvents.length);
  assert.equal(first.activity.at(-1)?.title, "Ready for review");
  assert.equal(first.activity.length, 18);
  assert.ok(first.activity.some((item) => item.title === "Plan ready"));
  assert.ok(first.activity.some((item) => item.title === "Approval required"));
  assert.ok(first.activity.some((item) => item.title === "Tests recorded"));
  assert.ok(first.activity.some((item) => item.title === "Output captured"));
  assert.equal(
    first.activity.filter((item) => item.title === "Tool result").length,
    2,
  );

  assert.equal(inspector.pendingApproval, undefined);
  assert.deepEqual(
    inspector.tools.map(({ name, status, outputChunks }) => ({
      name,
      status,
      outputChunks,
    })),
    [
      {
        name: "Edit provider status card",
        status: "completed",
        outputChunks: 1,
      },
      {
        name: "Run focused tests",
        status: "completed",
        outputChunks: 1,
      },
    ],
  );
  assert.deepEqual(
    inspector.artifacts.map((artifact) => artifact.kind),
    ["plan", "diff", "test_result", "command_output"],
  );
  assert.equal(inspector.lastReceipt?.outcome, "completed");

  const projectEvent = initialSessionEvents.find(
    (event) => event.payload.type === "project_opened",
  );
  assert.match(
    projectEvent?.payload.type === "project_opened"
      ? projectEvent.payload.data.root
      : "",
    /Kiln User.*café kiln/,
  );
  const outputEvent = initialSessionEvents.find(
    (event) =>
      event.payload.type === "tool_output" &&
      event.payload.data.stream === "stdout",
  );
  assert.ok(
    outputEvent?.payload.type === "tool_output" &&
      outputEvent.payload.data.chunk.includes("\r\n"),
  );
});

test("projects an approved workspace edit and its safe diff artifact", () => {
  const stream = new ApplicationEventStream("task:edit", {
    taskId: "edit",
    clock: () => 42,
  });
  const events = [
    stream.append({
      type: "approval_requested",
      data: {
        approvalId: "approval:write-1",
        action: "write_file",
        resource: "src/lib.rs",
        reason: "Apply one atomic edit.",
      },
    }),
    stream.append({
      type: "approval_decided",
      data: {
        approvalId: "approval:write-1",
        decision: "approved",
        scope: "once",
      },
    }),
    stream.append({
      type: "artifact_published",
      data: {
        artifactId: "artifact:write-1",
        kind: "diff",
        label: "src/lib.rs",
      },
    }),
  ];

  const projection = projectEvents(events);
  assert.equal(projection.pendingApproval, undefined);
  assert.equal(projection.artifacts[0].label, "src/lib.rs");
  assert.ok(
    projection.activity.some((item) => item.title === "Approval required"),
  );
  assert.ok(projection.activity.some((item) => item.title === "Diff ready"));
});

test("keeps full workspace diffs out of durable event batches", async () => {
  const batches: ApplicationEvent[] = [];
  const sensitiveContent = "Authorization: Bearer do-not-persist\n";

  const result = await executeVisibleRepositoryTool(
    "project-preview",
    "tool-write-preview",
    {
      tool: "write_file",
      input: {
        path: "src/example.txt",
        content: sensitiveContent,
      },
    },
    async (events) => {
      batches.push(...events);
    },
  );

  assert.equal(result.tool, "write_file");
  assert.match(result.result.unifiedDiff, /Authorization: Bearer/);
  const durableJson = JSON.stringify(batches);
  assert.doesNotMatch(durableJson, /Authorization: Bearer/);
  assert.doesNotMatch(durableJson, /unifiedDiff/);
  assert.ok(
    batches.some(
      (event) =>
        event.type === "artifact_published" && event.data.kind === "diff",
    ),
  );
});

test("projects an ordered provider turn through application events", () => {
  let clock = 1_753_731_600_000;
  const stream = new ApplicationEventStream("task:test", {
    taskId: "test",
    clock: () => clock++,
    eventId: (sequence) => `event-${sequence}`,
  });
  const metadata = {
    causationId: "command-1",
    correlationId: "turn-1",
  };
  const events = [
    stream.append(
      {
        type: "message_added",
        data: { messageId: "user-1", role: "user", content: "Check it." },
      },
      metadata,
    ),
    stream.append(
      { type: "turn_started", data: { turnId: "turn-1" } },
      metadata,
    ),
    stream.append(
      {
        type: "message_delta",
        data: { messageId: "assistant-1", delta: "All " },
      },
      metadata,
    ),
    stream.append(
      {
        type: "message_delta",
        data: { messageId: "assistant-1", delta: "green." },
      },
      metadata,
    ),
    stream.append(
      {
        type: "message_completed",
        data: {
          messageId: "assistant-1",
          model: "local-model",
          content: "All green.",
          finishReason: "stop",
          usage: { totalTokens: 12 },
        },
      },
      metadata,
    ),
    stream.append(
      {
        type: "turn_receipt",
        data: {
          turnId: "turn-1",
          outcome: "completed",
          summary: "Checks passed.",
        },
      },
      metadata,
    ),
  ];

  const projection = projectEvents(events);
  assert.equal(projection.messages.at(-1)?.content, "All green.");
  assert.equal(projection.messages.at(-1)?.note, "12 tokens");
  assert.equal(projection.running, false);
  assert.equal(projection.status, "completed");
});

test("projects repository identity, branch, status, defaults, and workspace", () => {
  const stream = new ApplicationEventStream("project:kiln", {
    clock: () => 1_753_731_600_000,
    eventId: (sequence) => `project-event-${sequence}`,
  });
  const events = [
    stream.append({
      type: "project_opened",
      data: {
        projectId: "project-kiln",
        root: "D:\\Projects\\kiln",
        displayName: "kiln",
        branch: "main",
        head: "0123456789abcdef",
        status: {
          staged: 1,
          modified: 2,
          untracked: 3,
          conflicts: 0,
          ahead: 1,
          behind: 0,
        },
        defaults: {
          provider: "openai",
          model: "gpt-5",
          verificationProfile: "quick",
        },
      },
    }),
    stream.append({
      type: "workspace_ready",
      data: {
        workspaceId: "workspace:direct:project-kiln",
        projectId: "project-kiln",
        path: "D:\\Projects\\kiln",
        isolated: false,
      },
    }),
  ];

  const projection = projectProjectEvents(events);
  assert.equal(projection.project?.branch, "main");
  assert.equal(projection.project?.status.modified, 2);
  assert.equal(projection.project?.defaults.model, "gpt-5");
  assert.equal(
    projection.workspace?.workspaceId,
    "workspace:direct:project-kiln",
  );
  const serialized = JSON.stringify(projection).toLowerCase();
  assert.ok(!serialized.includes("credential"));
  assert.ok(!serialized.includes("apikey"));
});

test("ignores late provider mutations after a cancellation receipt", () => {
  const stream = new ApplicationEventStream("task:cancelled", {
    taskId: "cancelled",
    clock: () => 1_753_731_600_000,
    eventId: (sequence) => `cancel-event-${sequence}`,
  });
  const events = [
    stream.append({
      type: "turn_started",
      data: { turnId: "cancelled-turn" },
    }),
    stream.append({
      type: "message_delta",
      data: { messageId: "assistant-1", delta: "Visible" },
    }),
    stream.append({
      type: "turn_receipt",
      data: {
        turnId: "cancelled-turn",
        outcome: "cancelled",
        summary: "Stopped by the user.",
      },
    }),
    stream.append({
      type: "message_delta",
      data: { messageId: "assistant-1", delta: " late" },
    }),
    stream.append({
      type: "message_completed",
      data: {
        messageId: "assistant-1",
        model: "ignored",
        content: "Visible late",
        usage: {},
      },
    }),
  ];

  const projection = projectEvents(events);
  assert.equal(projection.status, "cancelled");
  assert.equal(projection.messages[0].content, "Visible");
  assert.equal(projection.messages[0].model, "streaming");
  assert.equal(projection.lastSequence, 5);
});

test("normalizes desktop stream messages into application event batches", () => {
  const request = {
    provider: "local" as const,
    credentials: {},
    model: "qwen",
    messages: [{ role: "user" as const, content: "Stream it." }],
  };
  const context = {
    turnId: "turn-stream",
    assistantMessageId: "assistant-stream",
  };
  const delta = applicationEventsFromStream(
    {
      type: "provider",
      data: {
        event: { type: "message_delta", data: { delta: "Hello" } },
      },
    },
    request,
    context,
  );
  const completed = applicationEventsFromStream(
    {
      type: "provider",
      data: {
        event: {
          type: "message_completed",
          data: {
            response: {
              provider: "local",
              model: "qwen",
              content: "Hello world",
              usage: { totalTokens: 4 },
            },
          },
        },
      },
    },
    request,
    context,
  );
  const cancelled = applicationEventsFromStream(
    {
      type: "provider",
      data: {
        event: {
          type: "cancelled",
          data: { reason: "Stopped by the user." },
        },
      },
    },
    request,
    context,
  );

  assert.equal(delta.terminal, false);
  assert.equal(delta.events[0].type, "message_delta");
  assert.equal(completed.terminal, true);
  assert.deepEqual(
    completed.events.map((event) => event.type),
    ["message_completed", "turn_receipt"],
  );
  assert.equal(cancelled.terminal, true);
  assert.deepEqual(cancelled.events, [
    {
      type: "turn_receipt",
      data: {
        turnId: "turn-stream",
        outcome: "cancelled",
        summary: "Stopped by the user.",
      },
    },
  ]);
});

test("rejects breaking versions and sequence gaps", () => {
  const invalidVersion = {
    ...initialSessionEvents[0],
    schemaVersion: APPLICATION_CONTRACT_VERSION + 1,
  };
  assert.throws(() => validateEvent(invalidVersion), /Unsupported application contract/);

  const gap = [
    initialSessionEvents[0],
    { ...initialSessionEvents[1], sequence: 3 },
  ];
  assert.throws(() => validateOrderedEvents(gap), /expected 2/);
});

test("makes events visible only after persistence", async () => {
  const history = new DurableTaskHistory("task:durable", "durable");
  let persistedSequence = 0;

  const pending = history.append(
    [{ type: "task_created", data: { title: "Durable task" } }],
    { causationId: "command-1" },
    async (events) => {
      assert.equal(history.projection.lastSequence, 0);
      persistedSequence = events[0].sequence;
    },
  );

  const projection = await pending;
  assert.equal(persistedSequence, 1);
  assert.equal(projection.lastSequence, 1);
  assert.equal(history.events.length, 1);
});

test("resets the sequencer when persistence fails", async () => {
  const history = new DurableTaskHistory("task:retry", "retry");
  await history.append(
    [{ type: "task_created", data: { title: "Retry task" } }],
    {},
    async () => {},
  );

  await assert.rejects(
    history.append(
      [
        {
          type: "task_status_changed",
          data: { status: "running" },
        },
      ],
      {},
      async () => {
        throw new Error("disk unavailable");
      },
    ),
    /disk unavailable/,
  );
  assert.equal(history.projection.lastSequence, 1);

  let retrySequence = 0;
  await history.append(
    [
      {
        type: "task_status_changed",
        data: { status: "running" },
      },
    ],
    {},
    async (events) => {
      retrySequence = events[0].sequence;
    },
  );
  assert.equal(retrySequence, 2);
});
