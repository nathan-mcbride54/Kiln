import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const BODY_LIMIT = 256 * 1024;

function sse(...events) {
  return events
    .map((event) => `data: ${JSON.stringify(event)}\n\n`)
    .join("");
}

test("keeps hosted provider diagnostics bounded and destination-safe", async () => {
  const originalFetch = globalThis.fetch;
  const calls = [];
  let mode = "compatible";
  let toolArguments = { value: "ok" };

  globalThis.fetch = async (input, init = {}) => {
    const url = String(input);
    const body = init.body ? JSON.parse(String(init.body)) : undefined;
    calls.push({ body, init, url });

    if (mode === "reject") {
      return new Response("upstream-secret-body", { status: 401 });
    }
    if (mode === "redirect") {
      return new Response(null, {
        status: 302,
        headers: { location: "https://attacker.invalid/models" },
      });
    }
    if (mode === "forbid-upstream") {
      throw new Error("Invalid requests must not reach an upstream");
    }
    if (url === "https://api.openai.com/v1/models") {
      return Response.json({ data: [{ id: "gpt-test" }] });
    }
    if (url === "https://api.anthropic.com/v1/models") {
      return Response.json({ data: [{ id: "claude-test" }] });
    }
    if (url === "https://api.openai.com/v1/responses" && !body.tools) {
      assert.equal(body.max_output_tokens, 16);
      assert.equal(body.store, false);
      assert.equal(body.stream, true);
      return new Response(
        sse(
          { type: "response.output_text.delta", delta: "OK" },
          { type: "response.completed" },
        ),
        { headers: { "content-type": "text/event-stream" } },
      );
    }
    if (url === "https://api.openai.com/v1/responses") {
      assert.equal(body.parallel_tool_calls, false);
      assert.equal(body.store, false);
      assert.equal(body.tools[0].strict, true);
      assert.deepEqual(body.tool_choice, {
        type: "function",
        name: "kiln_capability_probe",
      });
      return new Response(
        sse(
          {
            type: "response.output_item.added",
            item: {
              type: "function_call",
              name: "kiln_capability_probe",
              arguments: "",
            },
          },
          {
            type: "response.function_call_arguments.delta",
            delta: JSON.stringify(toolArguments),
          },
          { type: "response.completed" },
        ),
        { headers: { "content-type": "text/event-stream" } },
      );
    }
    if (url === "https://api.anthropic.com/v1/messages" && !body.tools) {
      assert.equal(body.max_tokens, 8);
      assert.equal(body.stream, true);
      return new Response(
        sse(
          { type: "message_start" },
          {
            type: "content_block_delta",
            delta: { type: "text_delta", text: "OK" },
          },
          { type: "message_stop" },
        ),
        { headers: { "content-type": "text/event-stream" } },
      );
    }
    if (url === "https://api.anthropic.com/v1/messages") {
      assert.deepEqual(body.tool_choice, {
        type: "tool",
        name: "kiln_capability_probe",
        disable_parallel_tool_use: true,
      });
      return new Response(
        sse(
          { type: "message_start" },
          {
            type: "content_block_start",
            content_block: {
              type: "tool_use",
              name: "kiln_capability_probe",
              input: {},
            },
          },
          {
            type: "content_block_delta",
            delta: {
              type: "input_json_delta",
              partial_json: JSON.stringify(toolArguments),
            },
          },
          { type: "message_stop" },
        ),
        { headers: { "content-type": "text/event-stream" } },
      );
    }
    throw new Error(`Unexpected upstream destination: ${url}`);
  };

  try {
    const workerUrl = new URL("../dist/server/index.js", import.meta.url);
    workerUrl.searchParams.set("provider-route-test", `${process.pid}-${Date.now()}`);
    const { default: worker } = await import(workerUrl.href);
    const env = {
      ASSETS: {
        fetch: async () => new Response("Not found", { status: 404 }),
      },
    };
    const context = {
      waitUntil() {},
      passThroughOnException() {},
    };
    const providerRequest = (body, headers = {}) =>
      new Request("http://localhost/api/provider", {
        method: "POST",
        headers: { "content-type": "application/json", ...headers },
        body: JSON.stringify(body),
      });

    const verified = await worker.fetch(
      providerRequest({
        action: "verify",
        provider: "openai",
        model: "gpt-test",
        apiKey: "sk-session",
      }),
      env,
      context,
    );
    assert.equal(verified.status, 200);
    const verifiedBody = await verified.json();
    assert.equal(verifiedBody.report.origin, "https://api.openai.com");
    assert.deepEqual(
      verifiedBody.report.probes.map((probe) => probe.status),
      ["passed", "passed", "passed", "passed", "passed"],
    );
    assert.deepEqual(
      calls.map((call) => call.url),
      [
        "https://api.openai.com/v1/models",
        "https://api.openai.com/v1/responses",
        "https://api.openai.com/v1/responses",
      ],
    );
    assert.ok(calls.every((call) => call.init.redirect === "manual"));
    assert.ok(
      calls.every(
        (call) => call.init.headers.Authorization === "Bearer sk-session",
      ),
    );

    calls.length = 0;
    const anthropicVerified = await worker.fetch(
      providerRequest({
        action: "verify",
        provider: "anthropic",
        model: "claude-test",
        apiKey: "sk-ant-session",
      }),
      env,
      context,
    );
    const anthropicBody = await anthropicVerified.json();
    assert.equal(anthropicBody.report.origin, "https://api.anthropic.com");
    assert.deepEqual(
      anthropicBody.report.probes.map((probe) => probe.status),
      ["passed", "passed", "passed", "passed", "passed"],
    );
    assert.deepEqual(
      calls.map((call) => call.url),
      [
        "https://api.anthropic.com/v1/models",
        "https://api.anthropic.com/v1/messages",
        "https://api.anthropic.com/v1/messages",
      ],
    );
    assert.ok(calls.every((call) => call.init.redirect === "manual"));
    assert.ok(
      calls.every(
        (call) =>
          call.init.headers["x-api-key"] === "sk-ant-session" &&
          call.init.headers["anthropic-version"] === "2023-06-01",
      ),
    );

    calls.length = 0;
    toolArguments = { value: "ok", unexpected: true };
    const malformedTool = await worker.fetch(
      providerRequest({
        action: "verify",
        provider: "openai",
        model: "gpt-test",
        apiKey: "sk-session",
      }),
      env,
      context,
    );
    const malformedBody = await malformedTool.json();
    assert.equal(malformedBody.report.probes[3].status, "passed");
    assert.equal(malformedBody.report.probes[4].status, "failed");

    mode = "redirect";
    calls.length = 0;
    const redirected = await worker.fetch(
      providerRequest({
        action: "test",
        provider: "openai",
        model: "gpt-test",
        apiKey: "sk-session",
      }),
      env,
      context,
    );
    const redirectedBody = await redirected.json();
    assert.equal(calls.length, 1);
    assert.equal(calls[0].init.redirect, "manual");
    assert.equal(redirectedBody.report.probes[1].status, "inconclusive");
    assert.equal(redirectedBody.report.probes[2].status, "failed");

    mode = "reject";
    calls.length = 0;
    const rejected = await worker.fetch(
      providerRequest({
        action: "test",
        provider: "openai",
        model: "gpt-test",
        apiKey: "sk-rejected",
      }),
      env,
      context,
    );
    const rejectedText = await rejected.text();
    assert.equal(rejected.status, 200);
    assert.doesNotMatch(rejectedText, /upstream-secret-body|sk-rejected/);

    mode = "forbid-upstream";
    calls.length = 0;
    const invalidField = await worker.fetch(
      providerRequest({
        action: "test",
        provider: "openai",
        model: 7,
        apiKey: "sk-session",
      }),
      env,
      context,
    );
    assert.equal(invalidField.status, 400);

    const declaredOversize = await worker.fetch(
      providerRequest(
        { action: "test", provider: "openai", apiKey: "sk-session" },
        { "content-length": String(BODY_LIMIT + 1) },
      ),
      env,
      context,
    );
    assert.equal(declaredOversize.status, 413);

    const streamedOversize = await worker.fetch(
      new Request("http://localhost/api/provider", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: "x".repeat(BODY_LIMIT + 1),
      }),
      env,
      context,
    );
    assert.equal(streamedOversize.status, 413);
    assert.equal(calls.length, 0);

    const workbench = await readFile(
      new URL("../app/workbench.tsx", import.meta.url),
      "utf8",
    );
    assert.match(workbench, /parsed\.protocol === "https:" \|\| loopback/);
    assert.match(workbench, /hostname === "localhost"/);
    assert.match(workbench, /Number\(ipv4\[0\]\) === 127/);
    assert.match(workbench, /hostname === "::1"/);
    assert.match(workbench, /!destination\.credentialsAllowed/);
  } finally {
    globalThis.fetch = originalFetch;
  }
});
