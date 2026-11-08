// What each stack's own checker said, read as the feature change reads it
// (feature-change.mjs's `runChecker`): clean or refused, its first error, and
// the warnings. The checkers themselves (`pw check`, `tsc --noEmit`,
// `svelte-check`) are not run here: `runChecker` is handed a runner that
// answers with their output, in the shapes the recorded run's results show
// (test/fixtures/recorded-run/feature.json). Each pin is a whole value.
import assert from "node:assert/strict";
import path from "node:path";
import test from "node:test";
import { runChecker } from "../feature-change.mjs";
import { REPO } from "../lib.mjs";

const ESC = "\u001b";
const red = (text) => `${ESC}[31m${text}${ESC}[0m`;

/** A runner that records how it was called and answers with `out`. */
function runner(out, ok) {
  const calls = [];
  const shell = (cmd, args, opts) => {
    calls.push({ cmd, args, opts });
    return { ok, status: ok ? 0 : 1, out };
  };
  return { calls, shell };
}

const check = (stack, out, ok = false) => runChecker(stack, runner(out, ok).shell);
const verdict = (clean, error, warnings = []) => ({ clean, error, warnings });

test("pw check is run on the standard library, the platform, the baseline's domain, lib and store, in that order", () => {
  const { calls, shell } = runner("", true);
  runChecker("pleris", shell);
  assert.equal(calls.length, 1);
  const { cmd, args, opts } = calls[0];
  assert.equal(cmd, path.join(process.env.CARGO_TARGET_DIR ?? path.join(REPO, "target"), "debug/pw"));
  assert.equal(args[0], "check");
  assert.equal(opts, undefined);
  const dirs = [...new Set(args.slice(1).map((f) => path.relative(REPO, path.dirname(f))))];
  assert.deepEqual(dirs, [
    "packages/pw-std",
    "packages/pw-platform-web",
    "benchmarks/baselines/pleris",
    "benchmarks/baselines/pleris/lib",
    "benchmarks/baselines/pleris/store",
  ]);
  assert.ok(args.slice(1).every((f) => f.endsWith(".pw")));
  assert.equal(args.filter((f) => f.endsWith("/pleris/domain.pw")).length, 1);
});

test("a clean pw check is clean with no error, and its warning lines are kept, trimmed and stripped of colour", () => {
  assert.deepEqual(check("pleris", "checked 31 files\n", true), verdict(true, null));
  const out = `checked 31 files\n  ${red("warning")}: [PW9001] an unused name\nwarning: another\nnote: not a warning\n`;
  assert.deepEqual(check("pleris", out, true), verdict(true, null, ["warning: [PW9001] an unused name", "warning: another"]));
});

test("a refused pw check reports its first error line, stripped of colour and trimmed, and its warnings", () => {
  const out = `${red("error")}: [PW0330] \`add_to_cart\`'s optimistic transition performs database.write<Carts>\n  --> lib/a.pw:3:1\nerror: a second one\nwarning: w\n`;
  assert.deepEqual(check("pleris", out), verdict(false, "error: [PW0330] `add_to_cart`'s optimistic transition performs database.write<Carts>", ["warning: w"]));
});

test("a refused pw check with no error line reports the output's first line as it was written, colour and all", () => {
  assert.deepEqual(check("pleris", `\n  ${red("the checker crashed")}  \nsecond line\n`), verdict(false, red("the checker crashed")));
  assert.deepEqual(check("pleris", ""), verdict(false, ""));
});

test("a pw check that succeeded is clean whatever its output says of errors", () => {
  assert.deepEqual(check("pleris", "error: stale line in a log\n", true), verdict(true, null));
});

test("tsc is run in the Next.js baseline through pnpm, and its output is read for the first 'error TSnnnn' line", () => {
  const { calls, shell } = runner("", true);
  runChecker("next-react", shell);
  assert.deepEqual(calls, [{ cmd: "pnpm", args: ["--silent", "typecheck"], opts: { cwd: path.join(REPO, "benchmarks/baselines/next-react") } }]);
  assert.deepEqual(check("next-react", "", true), verdict(true, null));
  const out = `> tsc --noEmit\n${red("app/page.tsx(3,5): error TS2322: Type 'string' is not assignable to type 'number'.")}\napp/b.tsx(1,1): error TS2304: Cannot find name 'x'.\n`;
  assert.deepEqual(check("next-react", out), verdict(false, "app/page.tsx(3,5): error TS2322: Type 'string' is not assignable to type 'number'."));
});

test("tsc output with no TS error line is reported by its first line as written; tsc has no warnings", () => {
  assert.deepEqual(check("next-react", `\n  ${red("sh: tsc: not found")}\nmore\n`), verdict(false, red("sh: tsc: not found")));
  assert.deepEqual(check("next-react", "warning TS6385: deprecated\n", true), verdict(true, null));
});

test("svelte-check is run in the SvelteKit baseline in machine output; zero errors from a run that succeeded is clean", () => {
  const { calls, shell } = runner("", true);
  runChecker("sveltekit", shell);
  assert.deepEqual(calls, [
    { cmd: "pnpm", args: ["--silent", "typecheck", "--output", "machine"], opts: { cwd: path.join(REPO, "benchmarks/baselines/sveltekit") } },
  ]);
  assert.deepEqual(check("sveltekit", '1730000000 START "/work/sveltekit"\n1730000001 COMPLETED 40 FILES 0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS\n', true), verdict(true, null));
});

test("svelte-check's warnings are its WARNING lines without their time stamp, and a clean run keeps them", () => {
  const out = [
    '1730000000 START "/work/sveltekit"',
    '1730000001 WARNING "src/routes/stores/[storeId]/+page.svelte" 12:33 "This reference only captures the initial value of `data`."',
    '1730000002 WARNING "src/lib/x.svelte" 3:1 "A11y: <img> needs alt"  ',
    "1730000003 COMPLETED 40 FILES 0 ERRORS 2 WARNINGS 2 FILES_WITH_PROBLEMS",
    "",
  ].join("\n");
  assert.deepEqual(
    check("sveltekit", out, true),
    verdict(true, null, [
      'WARNING "src/routes/stores/[storeId]/+page.svelte" 12:33 "This reference only captures the initial value of `data`."',
      'WARNING "src/lib/x.svelte" 3:1 "A11y: <img> needs alt"',
    ]),
  );
});

test("svelte-check with errors is refused by its first ERROR line, never the summary's ERRORS line", () => {
  const out = [
    '1730000000 START "/work/sveltekit"',
    "1730000003 COMPLETED 40 FILES 2 ERRORS 0 WARNINGS 1 FILES_WITH_PROBLEMS",
    '1730000004 ERROR "src/routes/a.svelte" 4:5 "Type \'string\' is not assignable to type \'number\'."  ',
    '1730000005 ERROR "src/routes/b.svelte" 1:1 "Cannot find name \'x\'."',
    "",
  ].join("\n");
  const want = verdict(false, '1730000004 ERROR "src/routes/a.svelte" 4:5 "Type \'string\' is not assignable to type \'number\'."');
  assert.deepEqual(check("sveltekit", out, true), want);
  assert.deepEqual(check("sveltekit", out), want);
});

test("svelte-check with a summary of errors and no ERROR line is refused by the summary; with no summary, by the first line", () => {
  assert.deepEqual(check("sveltekit", "1730000003 COMPLETED 40 FILES 2 ERRORS 0 WARNINGS 1 FILES_WITH_PROBLEMS\n"), verdict(false, "2 ERRORS 0 WARNINGS"));
  assert.deepEqual(check("sveltekit", "\n  pnpm: command not found\nmore\n"), verdict(false, "pnpm: command not found"));
  assert.deepEqual(check("sveltekit", "pnpm: command not found\n", true), verdict(false, "pnpm: command not found"));
});

test("svelte-check that failed to run is refused even when its summary counts no errors", () => {
  const out = "1730000003 COMPLETED 40 FILES 0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS\n";
  assert.deepEqual(check("sveltekit", out), verdict(false, "0 ERRORS 0 WARNINGS"));
});

test("a stack the feature change does not know is refused with a TypeError", () => {
  assert.throws(() => check("remix", ""), TypeError);
});
