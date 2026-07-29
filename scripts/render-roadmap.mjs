import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const sourcePath = resolve(root, "product/roadmap.json");
const checkOnly = process.argv.includes("--check");
const allowedStatuses = new Set([
  "discovery",
  "planned",
  "in_progress",
  "blocked",
  "beta",
  "done",
  "deferred",
]);

const roadmap = JSON.parse(await readFile(sourcePath, "utf8"));
validate(roadmap);

const outputs = new Map([
  [resolve(root, "ROADMAP.md"), renderMarkdown(roadmap)],
  [resolve(root, "app/roadmap.generated.ts"), renderWebModule(roadmap)],
  [
    resolve(root, "desktop/src/lib/roadmap.generated.ts"),
    renderDesktopModule(roadmap),
  ],
]);

let stale = false;
for (const [path, content] of outputs) {
  if (checkOnly) {
    const existing = await readFile(path, "utf8").catch(() => "");
    if (existing !== content) {
      console.error(`${path.slice(root.length + 1)} is stale`);
      stale = true;
    }
  } else {
    await writeFile(path, content, "utf8");
    console.log(`rendered ${path.slice(root.length + 1)}`);
  }
}

if (stale) {
  console.error("Run `npm run roadmap:render` and commit the generated files.");
  process.exitCode = 1;
}

function validate(data) {
  assert.equal(data.schemaVersion, 1, "unsupported roadmap schema");
  assert.match(data.revision, /^\d+\.\d+$/);
  assert.match(data.lastReviewed, /^\d{4}-\d{2}-\d{2}$/);
  assert.ok(data.phases.some((phase) => phase.id === data.currentHorizon));

  const phaseIds = new Set();
  const itemIds = new Set();
  for (const phase of data.phases) {
    assert.match(phase.id, /^H\d+$/);
    assert.ok(!phaseIds.has(phase.id), `duplicate phase ${phase.id}`);
    phaseIds.add(phase.id);
    assert.ok(phase.exitGates.length > 0, `${phase.id} needs exit gates`);

    for (const item of phase.items) {
      assert.match(item.id, new RegExp(`^${phase.id}-\\d{3}$`));
      assert.ok(!itemIds.has(item.id), `duplicate item ${item.id}`);
      itemIds.add(item.id);
      assert.ok(allowedStatuses.has(item.status), `${item.id} has invalid status`);
      assert.match(item.priority, /^P[0-2]$/);
      assert.ok(item.platforms.length > 0, `${item.id} needs platforms`);
      assert.ok(
        item.acceptanceCriteria.length > 0,
        `${item.id} needs acceptance criteria`,
      );
      assert.match(item.lastReviewed, /^\d{4}-\d{2}-\d{2}$/);
      assert.ok(item.changeNote, `${item.id} needs a change note`);
    }
  }

  for (const phase of data.phases) {
    for (const item of phase.items) {
      for (const dependency of item.dependencies) {
        assert.ok(itemIds.has(dependency), `${item.id} has unknown dependency ${dependency}`);
      }
    }
  }
  for (const focusId of data.focus) {
    assert.ok(itemIds.has(focusId), `unknown focus item ${focusId}`);
  }
}

function progressFor(phase) {
  return Math.round(
    (phase.items.filter((item) => item.status === "done").length /
      phase.items.length) *
      100,
  );
}

function allItems(data) {
  return data.phases.flatMap((phase) => phase.items);
}

function renderMarkdown(data) {
  const itemsById = new Map(allItems(data).map((item) => [item.id, item]));
  const current = data.phases.find((phase) => phase.id === data.currentHorizon);
  const lines = [
    "# Kiln evolving roadmap",
    "",
    "> Generated from `product/roadmap.json`. Edit the structured source, then run",
    "> `npm run roadmap:render`. Use `npm run roadmap:check` in CI.",
    "",
    "| Revision | Last reviewed | Current horizon | Launch platforms | Later platforms |",
    "|---|---|---|---|---|",
    `| ${data.revision} | ${data.lastReviewed} | ${data.currentHorizon} — ${current.title} | ${data.releaseTargets.join(" and ")} | ${data.laterPlatforms.join(", ")} |`,
    "",
    "Kiln measures progress through complete, reliable user journeys. An item is",
    "done only when every acceptance criterion passes on its target platforms.",
    "",
    "## Current focus",
    "",
    "| ID | Priority | Status | Outcome |",
    "|---|---:|---|---|",
    ...data.focus.map((id) => {
      const item = itemsById.get(id);
      return `| ${item.id} | ${item.priority} | \`${item.status}\` | ${escapeTable(item.outcome)} |`;
    }),
    "",
    `**Next review trigger:** ${data.nextReviewTrigger}.`,
    "",
    "## Horizon overview",
    "",
    "| Horizon | Status | Progress | Outcome |",
    "|---|---|---:|---|",
    ...data.phases.map(
      (phase) =>
        `| ${phase.id} — ${phase.title} | ${phase.statusLabel} | ${progressFor(phase)}% | ${escapeTable(phase.outcome)} |`,
    ),
    "",
  ];

  for (const phase of data.phases) {
    lines.push(
      `## ${phase.id} — ${phase.title}`,
      "",
      `**Lane:** ${phase.lane} · **Status:** ${phase.statusLabel} · **Timeframe:** ${phase.timeframe}`,
      "",
      `**Outcome:** ${phase.outcome}`,
      "",
      "### Work items",
      "",
      "| ID | Priority | Status | Platforms | Dependencies | Deliverable |",
      "|---|---:|---|---|---|---|",
      ...phase.items.map(
        (item) =>
          `| ${item.id} | ${item.priority} | \`${item.status}\` | ${item.platforms.join(", ")} | ${
            item.dependencies.length ? item.dependencies.join(", ") : "—"
          } | ${escapeTable(item.title)} |`,
      ),
      "",
      "### Acceptance criteria",
      "",
    );

    for (const item of phase.items) {
      lines.push(
        `#### ${item.id} — ${item.title}`,
        "",
        item.outcome,
        "",
        ...item.acceptanceCriteria.map((criterion) => `- ${criterion}`),
        "",
        `Last reviewed ${item.lastReviewed}. ${item.changeNote}`,
        "",
      );
    }

    lines.push(
      "### Exit gates",
      "",
      ...phase.exitGates.map((gate) => `- ${gate}`),
      "",
    );
  }

  lines.push(
    "## Risk register",
    "",
    "| ID | Severity | Status | Risk | Mitigation |",
    "|---|---|---|---|---|",
    ...data.risks.map(
      (risk) =>
        `| ${risk.id} | ${risk.severity} | ${risk.status} | ${escapeTable(risk.risk)} | ${escapeTable(risk.mitigation)} |`,
    ),
    "",
    "## Decision queue",
    "",
    "| ID | Status | Decision | Review point | Reason |",
    "|---|---|---|---|---|",
    ...data.decisions.map(
      (decision) =>
        `| ${decision.id} | ${decision.status} | ${escapeTable(decision.decision)} | ${decision.reviewAt} | ${escapeTable(decision.reason)} |`,
    ),
    "",
    "## Success metrics",
    "",
    ...data.metrics.map((metric) => `- ${metric}.`),
    "",
    "## Roadmap policy",
    "",
    ...Object.entries(data.statusDefinitions).map(
      ([status, definition]) => `- \`${status}\`: ${definition}`,
    ),
    "- New provider-specific behavior must update the capability contract.",
    "- Architectural changes require a short decision record.",
    "- Deferred work remains visible with its reason.",
    "- Progress is measured by completed user journeys and reliability gates, not feature count.",
    "",
    "## Change history",
    "",
  );

  for (const entry of data.changeLog) {
    lines.push(
      `### ${entry.date} — revision ${entry.revision}`,
      "",
      entry.summary,
      "",
      ...entry.changes.map((change) => `- ${change}`),
      "",
    );
  }

  return `${lines.join("\n").trim()}\n`;
}

function renderWebModule(data) {
  const summary = data.phases.map((phase) => {
    const activeItems = phase.items
      .filter((item) => item.status !== "done")
      .slice(0, 3);
    const scope = (activeItems.length ? activeItems : phase.items.slice(0, 3)).map(
      (item) => item.title,
    );
    return {
      id: phase.id,
      title: phase.title,
      status: phase.statusLabel,
      progress: progressFor(phase),
      outcome: phase.outcome,
      now: scope,
      gates: phase.exitGates.slice(0, 3),
    };
  });

  return `// Generated from product/roadmap.json by scripts/render-roadmap.mjs.\n` +
    `// Do not edit this file directly.\n\n` +
    `export const roadmapRevision = ${JSON.stringify(data.revision)};\n` +
    `export const roadmapLastReviewed = ${JSON.stringify(data.lastReviewed)};\n` +
    `export const roadmapCurrentHorizon = ${JSON.stringify(data.currentHorizon)};\n` +
    `export const roadmap = ${JSON.stringify(summary, null, 2)} as const;\n`;
}

function renderDesktopModule(data) {
  const summary = data.phases.map((phase) => {
    const activeItems = phase.items
      .filter((item) => item.status !== "done")
      .slice(0, 3);
    const outcomes = (
      activeItems.length ? activeItems : phase.items.slice(0, 3)
    ).map((item) => item.title);

    return {
      id: phase.id,
      title: phase.title,
      horizon: phase.timeframe,
      status: phase.lane,
      summary: phase.outcome,
      outcomes,
    };
  });

  return `// Generated from product/roadmap.json by scripts/render-roadmap.mjs.\n` +
    `// Do not edit this file directly.\n\n` +
    `export type RoadmapLane = "now" | "next" | "later";\n\n` +
    `export interface RoadmapPhase {\n` +
    `  id: string;\n` +
    `  title: string;\n` +
    `  horizon: string;\n` +
    `  status: RoadmapLane;\n` +
    `  summary: string;\n` +
    `  outcomes: readonly string[];\n` +
    `}\n\n` +
    `export const roadmapRevision = ${JSON.stringify(data.revision)};\n` +
    `export const roadmapLastReviewed = ${JSON.stringify(data.lastReviewed)};\n` +
    `export const roadmapCurrentHorizon = ${JSON.stringify(data.currentHorizon)};\n` +
    `export const roadmap: readonly RoadmapPhase[] = ${JSON.stringify(summary, null, 2)};\n`;
}

function escapeTable(value) {
  return String(value).replaceAll("|", "\\|").replaceAll("\n", " ");
}
