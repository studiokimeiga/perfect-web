// ADR-0346 (argument invariants): a value of an opaque type with a `where`
// invariant exists only if it passed the invariant. What a browser sends is a
// claim, so the host decodes it through the checks its type states:
//   - a page's parameters, as its address gives them, at both ends of each
//     invariant, refused 400 and why before the page is rendered;
//   - a command's arguments, at the top and inside a list's item, a record's
//     field and a case's payload, refused 400 and why before the command runs,
//     nothing kept, where a value just inside each bound commits;
//   - a press through the page sends only what the page's own checked
//     constructors build; one whose request is altered on the way is refused
//     all the same, and the press says so where it was.
// `examples/demo/invariants.pw`: `Stars` 1 to 5, `Note` at most 20 code
// points, `Code` 2 to 8.
import { expect, test } from "@playwright/test";

const PAGE = "/demo/invariants/ab";

/** The page, its handlers attached (the runtime's `data-pw-ready`). */
async function ready(page) {
  await page.goto(PAGE);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  await expect(page.locator("h1")).toHaveText("Rate a code");
}
const STARS = "`demo.invariants.Stars` holds `value >= 1 & value <= 5`";
const NOTE = "`demo.invariants.Note` holds `String.length(value) <= 20`";
const CODE =
  "`demo.invariants.Code` holds `String.length(value) >= 2 & String.length(value) <= 8`";

test("a page's parameters are held where its address gives them", async ({ request }) => {
  for (const path of [
    "/demo/invariants/ab",
    "/demo/invariants/abcdefgh",
    `/demo/invariants/ab?note=${"n".repeat(20)}`,
    `/demo/invariants/ab?note=${encodeURIComponent("\u{1F600}".repeat(20))}`,
  ]) {
    const answer = await request.get(path);
    expect(answer.status(), path).toBe(200);
  }
  for (const [path, why] of [
    ["/demo/invariants/a", `parameter refused: value 1 is 1 code points long, and ${CODE}`],
    [
      "/demo/invariants/abcdefghi",
      `parameter refused: value 1 is 9 code points long, and ${CODE}`,
    ],
    [
      `/demo/invariants/ab?note=${"n".repeat(21)}`,
      `parameter refused: value 2's \`some\` is 21 code points long, and ${NOTE}`,
    ],
    [
      `/demo/invariants/ab?note=${encodeURIComponent("\u{1F600}".repeat(21))}`,
      `parameter refused: value 2's \`some\` is 21 code points long, and ${NOTE}`,
    ],
  ]) {
    const answer = await request.get(path);
    expect(answer.status(), path).toBe(400);
    expect(await answer.text(), path).toBe(why);
  }
});

test("a command's argument that breaks its invariant is refused before it runs", async ({
  page,
}) => {
  await ready(page);
  const press = (command, args, interaction) =>
    page.request.post(`/command/demo.invariants.${command}`, {
      data: args,
      headers: { "pw-interaction": interaction },
    });
  const none = { $case: "none" };
  for (const [command, args, why] of [
    ["rate", ["ab", 0, ""], `argument refused: value 2 is 0, and ${STARS}`],
    ["rate", ["ab", 6, ""], `argument refused: value 2 is 6, and ${STARS}`],
    ["rate", ["a", 3, ""], `argument refused: value 1 is 1 code points long, and ${CODE}`],
    [
      "rate",
      ["ab", 3, "\u{1F600}".repeat(21)],
      `argument refused: value 3 is 21 code points long, and ${NOTE}`,
    ],
    [
      "rate_all",
      [[{ stars: 3, note: none }, { stars: 9, note: none }]],
      `argument refused: value 1's \`[1].stars\` is 9, and ${STARS}`,
    ],
    [
      "rate_all",
      [[{ stars: 3, note: { $case: "some", value: "y".repeat(21) } }]],
      `argument refused: value 1's \`[0].note.some\` is 21 code points long, and ${NOTE}`,
    ],
    [
      "mark",
      [{ $case: "starred", value: 0 }],
      `argument refused: value 1's \`starred\` is 0, and ${STARS}`,
    ],
  ]) {
    const answer = await press(command, args, "forged");
    expect(answer.status(), JSON.stringify(args)).toBe(400);
    const body = await answer.json();
    expect(body, JSON.stringify(args)).toEqual({ committed: false, error: why });
  }
  // Nothing was kept of them: the interaction they named is free, and each
  // value just inside its bounds commits.
  for (const [command, args, interaction] of [
    ["rate", ["ab", 1, ""], "forged"],
    ["rate", ["abcdefgh", 5, "\u{1F600}".repeat(20)], "held-1"],
    ["rate_all", [[{ stars: 5, note: { $case: "some", value: "y".repeat(20) } }]], "held-2"],
    ["mark", [{ $case: "noted", value: "z".repeat(20) }], "held-3"],
  ]) {
    const answer = await press(command, args, interaction);
    expect(answer.status(), JSON.stringify(args)).toBe(202);
    expect(await answer.json(), JSON.stringify(args)).toMatchObject({
      committed: true,
      result: { $case: "ok" },
    });
  }
});

test("a press sends only what the page's own constructors build", async ({ page }) => {
  const sent = [];
  page.on("request", (r) => {
    if (r.url().includes("/command/")) sent.push(r.postData());
  });
  await ready(page);
  // Six stars are no `Stars`: the page says so, and sends nothing.
  await page.locator("#stars-6").click();
  await page.locator("#send").click();
  await expect(page.locator("#said")).toHaveText("Choose from 1 to 5 first.");
  // Nor is a note of 21.
  await page.locator("#stars-3").click();
  await page.locator("#typed").fill("x".repeat(21));
  await page.locator("#send").click();
  await expect(page.locator("#said")).toHaveText("A note is 20 characters at most.");
  expect(sent, "nothing was sent").toEqual([]);
  // Twenty is a note, and the press commits.
  await page.locator("#typed").fill("x".repeat(20));
  await page.locator("#send").click();
  await expect(page.locator("#said")).toHaveText("Sent.");
  expect(sent).toEqual([JSON.stringify(["ab", 3, "x".repeat(20)])]);
});

test("a press whose request is altered on the way is refused, and says so", async ({ page }) => {
  await ready(page);
  const uncaught = [];
  page.on("pageerror", (e) => uncaught.push(e.message));
  // The page sends three stars; the request arrives claiming six. The route
  // sends it on, keeps what the host answered, and gives the page that.
  // (Read here, not from the page's response: the runtime reads no body of
  // a refused command, and Chromium then gives a test none either.)
  let answered = null;
  await page.route("**/command/demo.invariants.rate", async (route) => {
    const response = await route.fetch({ postData: JSON.stringify(["ab", 6, ""]) });
    answered = { status: response.status(), body: await response.json() };
    await route.fulfill({ response });
  });
  await page.locator("#stars-3").click();
  await page.locator("#send").click();
  // Told where the press was, in the platform's words (ADR-0302), and the
  // page's own word is not "Sent.".
  const send = page.locator("#send");
  await expect(send).toHaveAttribute("data-pw-handler-error", "failed");
  expect(answered).toEqual({
    status: 400,
    body: { committed: false, error: `argument refused: value 2 is 6, and ${STARS}` },
  });
  await expect(page.locator("#send + .pw-refusal")).toHaveText("This did not work. Try again.");
  await expect(page.locator("#said")).toHaveText("");
  // A command the host refused as malformed is no answer the program gave:
  // the developer is told, as of an error nothing caught (ADR-0431).
  await expect.poll(() => uncaught.join("\n")).toBe("command demo.invariants.rate refused: HTTP 400");
});
