import assert from "node:assert/strict";
import test from "node:test";
import { outcomeTone, runCounts } from "../src/features/lab/outcome.ts";

test("a skipped run is neutral and does not count as a pass or failure", () => {
  assert.equal(outcomeTone("skip"), "plain");
  assert.deepEqual(runCounts({ games: [{ outcome: "skip" }] }), {
    passed: 0,
    warned: 0,
    console: 0,
    failed: 0,
  });
});

test("completed outcomes retain their counts beside skipped games", () => {
  const games = ["skip", "pass", "warn", "console", "fail", "skip"].map((outcome) => ({ outcome }));
  assert.deepEqual(runCounts({ games }), { passed: 1, warned: 1, console: 1, failed: 1 });
});
