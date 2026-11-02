// These controls run without a model API, browser, or production build.
import { after, test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { runAgent } from "../bench.mjs";
import { agentFailureKind, readAgentUsage, scoreAgentRun, validateAgentUsage } from "../agent-telemetry.mjs";

const made = [];
after(() => {
  for (const root of made) fs.rmSync(root, { recursive: true, force: true });
});

function usageFile(content) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "pw-bench-usage-"));
  made.push(root);
  const file = path.join(root, "usage.json");
  fs.writeFileSync(file, content);
  return file;
}

function fakeBox() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "pw-bench-adapter-"));
  made.push(root);
  const app = path.join(root, "app");
  fs.mkdirSync(app);
  return { root, app };
}

test("provider usage is kept without converting missing counters to zeros", () => {
  const raw = { model: "stub-small", input_tokens: 140, output_tokens: 12, cache_read_tokens: 30, turns: 2, cost_usd: 0.001 };
  assert.deepEqual(readAgentUsage(usageFile(JSON.stringify(raw))), {
    input_tokens: 140, output_tokens: 12, cache_read_tokens: 30, turns: 2, model: "stub-small", cost_usd: 0.001,
  });
  assert.deepEqual(validateAgentUsage({ input_tokens: 0, output_tokens: 0 }), { input_tokens: 0, output_tokens: 0 });
});

test("usage is optional when an adapter has not supplied a measurement", () => {
  const box = fakeBox();
  assert.equal(readAgentUsage(path.join(box.root, "unwritten.json")), null);
  const result = runAgent({}, "pleris", "true", box);
  assert.equal(result.ok, true);
  assert.equal(result.usage, null);
});

test("invalid measurements are refused instead of becoming benchmark evidence", () => {
  for (const raw of ["{", "[]", "{}", '{"input_tokens":2,"output_tokens":-1}', '{"input_tokens":1.5,"output_tokens":1}', '{"input_tokens":1,"output_tokens":2,"prompt":"secret"}', '{"input_tokens":1,"output_tokens":2,"cost_usd":-2}']) {
    assert.throws(() => readAgentUsage(usageFile(raw)), /BENCH_USAGE/);
  }
  assert.throws(() => readAgentUsage(usageFile("x".repeat(8193))), /regular file/);
});

test("usage symlinks are refused even if they point to a valid record", () => {
  const target = usageFile('{"input_tokens":1,"output_tokens":1}');
  const link = target + ".link";
  fs.symlinkSync(target, link);
  assert.throws(() => readAgentUsage(link), /regular file/);
});

test("a real stub adapter writes usage outside the graded app", () => {
  const box = fakeBox();
  const script = "node -e 'require(\"node:fs\").writeFileSync(process.env.BENCH_USAGE, JSON.stringify({input_tokens:42,output_tokens:7,turns:1,model:\"stub\"}))'";
  const result = runAgent({}, "pleris", script, box);
  assert.equal(result.ok, true, result.out);
  assert.deepEqual(result.usage, { input_tokens: 42, output_tokens: 7, turns: 1, model: "stub" });
  assert.ok(fs.existsSync(path.join(box.root, "AGENT_USAGE.json")));
  assert.equal(fs.existsSync(path.join(box.app, "AGENT_USAGE.json")), false);
  assert.deepEqual(scoreAgentRun(result, [{ name: "check", ok: true }]), { score: 1, failedAt: null });
});

test("a malformed sidecar turns an otherwise successful adapter run into ungraded infrastructure failure", () => {
  const result = runAgent({}, "pleris", "printf 'not-json' > \"$BENCH_USAGE\"", fakeBox());
  assert.equal(result.ok, false);
  assert.equal(result.failure_kind, "infra");
  assert.match(result.problem, /BENCH_USAGE/);
  assert.deepEqual(scoreAgentRun(result, []), { score: null, failedAt: "infra" });
});

test("ordinary agent failure is a zero, but an executable or timeout failure is ungraded", () => {
  assert.equal(agentFailureKind({ status: 2, signal: null, error_code: null }), "agent");
  assert.equal(agentFailureKind({ status: 75, signal: null, error_code: null }), "infra");
  assert.equal(agentFailureKind({ status: 127, signal: null, error_code: null }), "infra");
  assert.equal(agentFailureKind({ status: null, signal: "SIGTERM", error_code: "ETIMEDOUT" }), "infra");
  const ordinary = runAgent({}, "pleris", "exit 2", fakeBox());
  const temporary = runAgent({}, "pleris", "exit 75", fakeBox());
  assert.deepEqual(scoreAgentRun(ordinary, []), { score: 0, failedAt: "agent" });
  assert.deepEqual(scoreAgentRun(temporary, []), { score: null, failedAt: "infra" });
});

test("successful agent work needs real, passing grading stages", () => {
  assert.deepEqual(scoreAgentRun({ ok: true }, [{ name: "check", ok: true }]), { score: 1, failedAt: null });
  assert.deepEqual(scoreAgentRun({ ok: true }, [{ name: "check", ok: false }]), { score: 0, failedAt: "check" });
  assert.deepEqual(scoreAgentRun({ ok: true }, []), { score: null, failedAt: "infra" });
});
