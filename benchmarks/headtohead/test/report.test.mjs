// The report (report.mjs), pinned on the record: the kept run of the evidence
// of record (`just e14-head-to-head` at dbd37a8c, 2026-11-02, the recipe's
// `runs.json` and the sections beside it), trimmed to the fields the report
// reads (every field removed was removed only after the report came out
// byte for byte the same without it). Its rendering is the evidence's own
// report, 61 rows where Pleris loses and 32 where it is best, every table of
// both apps, in three engines, cold and warm, shaped and not, the servers'
// rows beside. A change to how the report is written must show here first.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { readSections } from "../head-to-head.mjs";
import { lossesFirst, renderReport } from "../report.mjs";

const RECORD = path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures/recorded-run");
const read = (name) => fs.readFileSync(path.join(RECORD, name), "utf8");
const recorded = () => JSON.parse(read("runs.json"));
const lines = (text) => text.split("\n");

test("the recorded run renders to the report of record, line for line", () => {
  assert.deepEqual(lines(renderReport(recorded(), readSections(RECORD))), lines(read("report.txt")));
});

test("the recorded run's losses and wins are the report's counts", () => {
  const { losses, wins, unclear } = lossesFirst(recorded(), readSections(RECORD));
  assert.deepEqual([losses.length, wins.length, unclear], [61, 32, 58]);
});
