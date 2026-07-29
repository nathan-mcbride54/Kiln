import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const sourcePath = resolve(root, "fixtures/sessions/complete-task-v1.json");
const outputPath = resolve(
  root,
  "desktop/src/lib/recorded-session.generated.ts",
);
const checkOnly = process.argv.includes("--check");
const events = JSON.parse(await readFile(sourcePath, "utf8"));

validate(events);

const content = `// Generated from fixtures/sessions/complete-task-v1.json.
// Run \`npm run fixtures:render\` after changing the canonical recording.

import type { EventEnvelope } from "./events.ts";

export const recordedSessionEvents = ${JSON.stringify(events, null, 2)} satisfies readonly EventEnvelope[];
`;

if (checkOnly) {
  const existing = await readFile(outputPath, "utf8").catch(() => "");
  if (existing !== content) {
    console.error("desktop/src/lib/recorded-session.generated.ts is stale");
    console.error("Run `npm run fixtures:render` and commit the generated file.");
    process.exitCode = 1;
  }
} else {
  await writeFile(outputPath, content, "utf8");
  console.log("rendered desktop/src/lib/recorded-session.generated.ts");
}

function validate(recording) {
  assert.ok(Array.isArray(recording), "recording must be an event array");
  assert.ok(recording.length > 0, "recording cannot be empty");

  const streamId = recording[0].streamId;
  const requiredTypes = new Set([
    "message_delta",
    "artifact_published",
    "tool_proposed",
    "approval_requested",
    "approval_decided",
    "tool_output",
    "tool_completed",
    "message_completed",
    "turn_receipt",
  ]);
  const artifactKinds = new Set();

  for (const [index, event] of recording.entries()) {
    assert.equal(event.schemaVersion, 1, `event ${index + 1} has wrong schema`);
    assert.equal(event.streamId, streamId, `event ${index + 1} changes stream`);
    assert.equal(event.sequence, index + 1, `event ${index + 1} is out of order`);
    assert.ok(event.eventId, `event ${index + 1} needs an eventId`);
    assert.ok(event.payload?.type, `event ${index + 1} needs a payload`);
    requiredTypes.delete(event.payload.type);
    if (event.payload.type === "artifact_published") {
      artifactKinds.add(event.payload.data.kind);
    }
  }

  assert.deepEqual(
    [...requiredTypes],
    [],
    `recording is missing event types: ${[...requiredTypes].join(", ")}`,
  );
  for (const kind of ["plan", "diff", "test_result"]) {
    assert.ok(artifactKinds.has(kind), `recording is missing ${kind} artifact`);
  }
  assert.equal(
    recording.at(-1).payload.type,
    "turn_receipt",
    "recording must end with a turn receipt",
  );
}
