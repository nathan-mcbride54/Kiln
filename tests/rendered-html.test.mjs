import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

async function render() {
  const workerUrl = new URL("../dist/server/index.js", import.meta.url);
  workerUrl.searchParams.set("test", `${process.pid}-${Date.now()}`);
  const { default: worker } = await import(workerUrl.href);

  return worker.fetch(
    new Request("http://localhost/", {
      headers: { accept: "text/html" },
    }),
    {
      ASSETS: {
        fetch: async () => new Response("Not found", { status: 404 }),
      },
    },
    {
      waitUntil() {},
      passThroughOnException() {},
    },
  );
}

test("server-renders the Kiln workbench", async () => {
  const response = await render();
  assert.equal(response.status, 200);
  assert.match(response.headers.get("content-type") ?? "", /^text\/html\b/i);

  const html = await response.text();
  assert.match(html, /Kiln/);
  assert.match(html, /Local-first agent workbench/);
  assert.match(html, /Workbench/);
  assert.match(html, /Providers/);
  assert.match(html, /Roadmap/);
  assert.match(html, /OpenAI/);
  assert.match(html, /Anthropic/);
  assert.match(html, /isolated worktree/);
});

test("keeps credentials ephemeral and cloud destinations fixed", async () => {
  const [workbench, providerRoute, roadmap] = await Promise.all([
    readFile(new URL("../app/workbench.tsx", import.meta.url), "utf8"),
    readFile(new URL("../app/api/provider/route.ts", import.meta.url), "utf8"),
    readFile(new URL("../ROADMAP.md", import.meta.url), "utf8"),
  ]);

  assert.match(workbench, /type="password"/);
  assert.match(workbench, /Session only/);
  assert.match(workbench, /Local server/);
  assert.match(workbench, /Nothing is written to browser storage/);
  assert.doesNotMatch(workbench, /localStorage|sessionStorage|indexedDB/i);

  assert.match(providerRoute, /https:\/\/api\.openai\.com\/v1\/responses/);
  assert.match(providerRoute, /https:\/\/api\.anthropic\.com\/v1\/messages/);
  assert.match(providerRoute, /store:\s*false/);
  assert.doesNotMatch(providerRoute, /console\.(log|debug|info)/);

  for (const horizon of ["H0", "H1", "H2", "H3", "H4", "H5", "H6", "H7"]) {
    assert.match(roadmap, new RegExp(`\\b${horizon}\\b`));
  }
  assert.match(roadmap, /Windows and Linux/);
  assert.match(roadmap, /macOS/);
});
