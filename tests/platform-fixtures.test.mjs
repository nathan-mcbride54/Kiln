import assert from "node:assert/strict";
import { once } from "node:events";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawn } from "node:child_process";
import test from "node:test";

test("handles spaces, Unicode paths, and both common line endings", async () => {
  const fixtureRoot = await mkdtemp(join(tmpdir(), "kiln fixtures café "));
  const fixturePath = join(fixtureRoot, "line endings Ω.txt");
  const mixedLines = "alpha\r\nbravo\ncharlie\r\n";

  try {
    await writeFile(fixturePath, mixedLines, "utf8");
    const roundTrip = await readFile(fixturePath, "utf8");

    assert.equal(roundTrip, mixedLines);
    assert.deepEqual(normalizeLines(roundTrip), [
      "alpha",
      "bravo",
      "charlie",
      "",
    ]);
  } finally {
    await rm(fixtureRoot, { recursive: true, force: true });
  }
});

test("cancels a long-running child process within the platform bound", async () => {
  const child = spawn(
    process.execPath,
    [
      "-e",
      "process.on('SIGTERM', () => process.exit(0)); setInterval(() => {}, 1000);",
    ],
    { stdio: "ignore", windowsHide: true },
  );

  await once(child, "spawn");
  const startedAt = performance.now();
  assert.equal(child.kill(), true);

  const [code, signal] = await Promise.race([
    once(child, "exit"),
    new Promise((_, reject) => {
      const timer = setTimeout(
        () => reject(new Error("child process did not stop within 5 seconds")),
        5_000,
      );
      timer.unref();
    }),
  ]);

  assert.ok(code !== null || signal !== null);
  assert.ok(performance.now() - startedAt < 5_000);
});

function normalizeLines(value) {
  return value.replaceAll("\r\n", "\n").split("\n");
}
