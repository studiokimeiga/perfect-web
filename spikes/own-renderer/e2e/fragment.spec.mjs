// A signal bound to the address's fragment (track `todomvc`, ADR-0367).
//
// `examples/demo/fragment.pw` binds the signal `tab` to its address's
// fragment: `#/` is "overview", `#/specs` "specs", `#/reviews` "reviews".
// What this shows, in each engine:
// - the server renders the first value, and the document says the binding;
// - the browser sets the signal to its address's form at start, and what
//   reads it, a text, an attribute and a block, follows;
// - a link, the back and forward buttons and a script each change it;
// - a fragment that is no form is the default;
// - the signals it does not bind keep their values, and a page that binds
//   no fragment ignores its address's.
import { expect, test } from "@playwright/test";

const PAGE = "/page/demo.fragment.FragmentPage~ui";

async function ready(page, hash = "") {
  await page.goto(PAGE + hash);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
}

/** What the page's runtime logged of the fragment. */
const followed = (page) =>
  page.evaluate(() => window.__pw.log.filter((line) => line.startsWith("fragment ")));

test("the server renders the first value, and the document names the binding", async ({
  request,
}) => {
  const html = await (await request.get(PAGE)).text();
  const markup = html.split('<script type="application/json"')[0];
  expect(markup).toMatch(/<p id="tab">[^<]*<!--pw:[^>]*-->overview<!--pw:/);
  expect(markup).not.toContain('id="specs"');
  const manifest = JSON.parse(
    html.split('<script type="application/json" id="pw-parts">')[1].split("</script>")[0],
  );
  expect(manifest.fragment).toEqual({
    signal: "tab",
    forms: [
      { form: "/", value: "overview" },
      { form: "/specs", value: "specs" },
      { form: "/reviews", value: "reviews" },
    ],
  });
  // The control: a page that binds none says none.
  const bind = await (await request.get("/page/demo.bind.BindPage~ui")).text();
  expect(bind).not.toContain('"fragment"');
});

test("an address with a form shows it at start, and what reads the signal follows", async ({
  page,
}) => {
  await ready(page, "#/specs");
  await expect(page.locator("#tab")).toHaveText("specs");
  await expect(page.locator("#marked")).toHaveClass("specs-open");
  await expect(page.locator("#specs")).toBeVisible();
  expect(await followed(page)).toEqual(['fragment #/specs -> tab "specs"']);
  // Another form, in its own page.
  await ready(page, "#/reviews");
  await expect(page.locator("#tab")).toHaveText("reviews");
  await expect(page.locator("#marked")).toHaveClass("specs-shut");
  await expect(page.locator("#specs")).toHaveCount(0);
});

test("the default is no change: an address with none, or with the first form", async ({
  page,
}) => {
  await ready(page);
  await expect(page.locator("#tab")).toHaveText("overview");
  expect(await followed(page)).toEqual([]);
  await ready(page, "#/");
  await expect(page.locator("#tab")).toHaveText("overview");
  expect(await followed(page)).toEqual([]);
});

test("a link to a form changes the signal, and what reads it", async ({ page }) => {
  await ready(page);
  await page.locator("#to-reviews").click();
  await expect(page.locator("#tab")).toHaveText("reviews");
  expect(page.url()).toMatch(/#\/reviews$/);
  await page.locator("#to-specs").click();
  await expect(page.locator("#tab")).toHaveText("specs");
  await expect(page.locator("#marked")).toHaveClass("specs-open");
  await expect(page.locator("#specs")).toBeVisible();
  await page.locator("#to-overview").click();
  await expect(page.locator("#tab")).toHaveText("overview");
  await expect(page.locator("#specs")).toHaveCount(0);
});

test("a link to the form the page is at makes no entry of its own", async ({ page }) => {
  // As the browser does for a link to the fragment it is at: the address is
  // replaced, not pushed, so back leaves the form (the first press's review).
  await ready(page);
  await page.locator("#to-reviews").click();
  await expect(page.locator("#tab")).toHaveText("reviews");
  await page.locator("#to-reviews").click();
  await expect(page.locator("#tab")).toHaveText("reviews");
  await page.goBack();
  await expect(page.locator("#tab")).toHaveText("overview");
});

test("a link to a form is followed in its press's own task", async ({ page }) => {
  // ADR-0399: not in the `hashchange` a browser dispatches a
  // task later, before which it often paints a frame without the change.
  await ready(page);
  // What a change of the signal needs, loaded by a first one: this holds the
  // order of tasks, not a module's network.
  await page.locator("#to-specs").click();
  await expect(page.locator("#tab")).toHaveText("specs");
  const seen = await page.evaluate(async () => {
    let hashchanges = 0;
    addEventListener("hashchange", () => hashchanges++);
    let taskRan = false;
    setTimeout(() => (taskRan = true), 0);
    document.querySelector("#to-reviews").click();
    // Microtasks only: a task, `hashchange` among them, cannot run in these.
    const tab = () => document.querySelector("#tab").textContent.trim();
    for (let i = 0; i < 200 && tab() !== "reviews"; i++) await Promise.resolve();
    const inPress = { tab: tab(), taskRan, hash: location.hash };
    // Tasks later, still no `hashchange`: the browser did not follow the link
    // itself, which it would have told in one (the first press's review).
    await new Promise((resolve) => setTimeout(resolve, 50));
    return { ...inPress, hashchanges };
  });
  expect(seen).toEqual({ tab: "reviews", taskRan: false, hash: "#/reviews", hashchanges: 0 });
  // A new entry, as the browser makes for a fragment: back is overview's
  // neighbour, specs.
  await page.goBack();
  await expect(page.locator("#tab")).toHaveText("specs");
});

test("back and forward follow the address", async ({ page }) => {
  await ready(page);
  await page.locator("#to-specs").click();
  await expect(page.locator("#tab")).toHaveText("specs");
  await page.locator("#to-reviews").click();
  await expect(page.locator("#tab")).toHaveText("reviews");
  await page.goBack();
  await expect(page.locator("#tab")).toHaveText("specs");
  await page.goBack();
  await expect(page.locator("#tab")).toHaveText("overview");
  await page.goForward();
  await expect(page.locator("#tab")).toHaveText("specs");
});

test("an address changed by a script is followed", async ({ page }) => {
  await ready(page);
  await page.evaluate(() => {
    location.hash = "#/specs";
  });
  await expect(page.locator("#tab")).toHaveText("specs");
  await page.evaluate(() => {
    location.hash = "#/reviews";
  });
  await expect(page.locator("#tab")).toHaveText("reviews");
});

test("a fragment that is no form is the default", async ({ page }) => {
  await ready(page, "#/nonsense");
  await expect(page.locator("#tab")).toHaveText("overview");
  // After a form: a fragment that is none takes the signal back.
  await page.locator("#to-specs").click();
  await expect(page.locator("#tab")).toHaveText("specs");
  await page.evaluate(() => {
    location.hash = "#/nonsense";
  });
  await expect(page.locator("#tab")).toHaveText("overview");
  // And the empty fragment, which no form is written for.
  await page.locator("#to-specs").click();
  await expect(page.locator("#tab")).toHaveText("specs");
  await page.evaluate(() => {
    location.hash = "";
  });
  await expect(page.locator("#tab")).toHaveText("overview");
});

test("a form is its characters exactly: the case and the slash count", async ({ page }) => {
  await ready(page, "#/SPECS");
  await expect(page.locator("#tab")).toHaveText("overview");
  await page.evaluate(() => {
    location.hash = "#specs";
  });
  await expect(page.locator("#tab")).toHaveText("overview");
  await page.evaluate(() => {
    location.hash = "#/specs";
  });
  await expect(page.locator("#tab")).toHaveText("specs");
});

test("the signals it does not bind keep their values", async ({ page }) => {
  await ready(page);
  await page.locator("#press").click();
  await page.locator("#press").click();
  await expect(page.locator("#presses")).toHaveText("2");
  await page.locator("#to-specs").click();
  await expect(page.locator("#tab")).toHaveText("specs");
  await expect(page.locator("#presses")).toHaveText("2");
  await page.goBack();
  await expect(page.locator("#tab")).toHaveText("overview");
  await expect(page.locator("#presses")).toHaveText("2");
});

test("a page that binds no fragment ignores its address's", async ({ page }) => {
  await page.goto("/page/demo.bind.BindPage~ui#/specs");
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  await expect(page.locator("#hello")).toHaveText("Hello, Ada");
  await page.evaluate(() => {
    location.hash = "#/reviews";
  });
  await page.locator("#grace").click();
  await expect(page.locator("#hello")).toHaveText("Hello, Grace");
  expect(await followed(page)).toEqual([]);
});
