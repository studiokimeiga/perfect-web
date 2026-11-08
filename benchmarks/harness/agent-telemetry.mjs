// E14: usage is an agent adapter's self-reported measurement, not a grader
// verdict. An unavailable measurement is null, never a fabricated zero.
import fs from "node:fs";

const COUNT_FIELDS = ["input_tokens", "output_tokens", "cache_read_tokens", "cache_write_tokens", "turns"];
const USAGE_FIELDS = new Set([...COUNT_FIELDS, "model", "cost_usd"]);

function requireRecord(value) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("BENCH_USAGE must contain a JSON object");
  }
}

function requireKnownFields(value) {
  if (Object.keys(value).some((key) => !USAGE_FIELDS.has(key))) {
    throw new Error("BENCH_USAGE contains unsupported fields");
  }
}

function count(value, field) {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new Error("BENCH_USAGE " + field + " must be a nonnegative safe integer");
  }
  return value;
}

function counts(value) {
  const entries = COUNT_FIELDS.filter((key) => Object.hasOwn(value, key)).map((key) => [key, count(value[key], key)]);
  const out = Object.fromEntries(entries);
  if (!Object.hasOwn(out, "input_tokens") || !Object.hasOwn(out, "output_tokens")) {
    throw new Error("BENCH_USAGE requires input_tokens and output_tokens");
  }
  return out;
}

function sizedModelName(value) {
  if (value.length === 0 || value.length > 128 || value.trim() !== value) {
    throw new Error("BENCH_USAGE model must be a nonempty trimmed string of at most 128 characters");
  }
  return value;
}

function modelName(value) {
  if (typeof value !== "string") throw new Error("BENCH_USAGE model must be a string");
  return sizedModelName(value);
}

function costAmount(value) {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) {
    throw new Error("BENCH_USAGE cost_usd must be a finite nonnegative number");
  }
  return value;
}

function extras(value) {
  const out = {};
  if (Object.hasOwn(value, "model")) out.model = modelName(value.model);
  if (Object.hasOwn(value, "cost_usd")) out.cost_usd = costAmount(value.cost_usd);
  return out;
}

export function validateAgentUsage(value) {
  requireRecord(value);
  requireKnownFields(value);
  return { ...counts(value), ...extras(value) };
}

function usageDescriptor(file) {
  if (!fs.constants.O_NOFOLLOW) {
    throw new Error("BENCH_USAGE needs O_NOFOLLOW support");
  }
  try {
    return fs.openSync(file, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW);
  } catch (error) {
    if (error.code === "ENOENT") return null;
    throw new Error("BENCH_USAGE must be a regular file");
  }
}

function checkUsageDescriptor(fd) {
  const stat = fs.fstatSync(fd);
  if (!stat.isFile() || stat.size > 8192) {
    throw new Error("BENCH_USAGE must be a regular file of at most 8192 bytes");
  }
}

function usageText(fd) {
  const buffer = Buffer.alloc(8193);
  let length = 0;
  while (length < buffer.length) {
    const read = fs.readSync(fd, buffer, length, buffer.length - length, length);
    if (read === 0) break;
    length += read;
  }
  if (length > 8192) throw new Error("BENCH_USAGE must be at most 8192 bytes");
  return buffer.subarray(0, length).toString("utf8");
}

function parseUsageJson(text) {
  try {
    return JSON.parse(text);
  } catch {
    throw new Error("BENCH_USAGE must contain valid JSON");
  }
}

export function readAgentUsage(file) {
  const fd = usageDescriptor(file);
  if (fd === null) return null;
  try {
    checkUsageDescriptor(fd);
    return validateAgentUsage(parseUsageJson(usageText(fd)));
  } finally {
    fs.closeSync(fd);
  }
}

// A run that outlives its time budget is the agent's failure, told apart as
// a timeout: finishing within the budget is part of what E14 measures, and an
// ungraded hang would favour the stack whose agents hang. 75 is an adapter's
// explicit temporary-infrastructure failure (a provider outage); 126/127 mean
// the wrapper could not execute its command; a command that could not be
// spawned, or was killed by a signal the harness did not send, has no status
// and is not the model's doing. Any other nonzero exit is the agent's.
export function agentFailureKind({ status, error_code = null }) {
  if (error_code === "ETIMEDOUT") return "timeout";
  if (status === null || [75, 126, 127].includes(status)) return "infra";
  return "agent";
}

// A null score means the run was not gradable. Do not count it as an agent
// failure in subsequent model comparisons; a timeout is one, scored 0.
function failedAgentScore(agentRun) {
  if (agentRun.failure_kind === "infra") return { score: null, failedAt: "infra" };
  if (agentRun.failure_kind === "timeout") return { score: 0, failedAt: "timeout" };
  return { score: 0, failedAt: "agent" };
}

export function scoreAgentRun(agentRun, stages) {
  if (!agentRun.ok) return failedAgentScore(agentRun);
  const failed = stages.find((stage) => !stage.ok);
  if (failed) return { score: 0, failedAt: failed.name };
  if (stages.length === 0) return { score: null, failedAt: "infra" };
  return { score: 1, failedAt: null };
}
