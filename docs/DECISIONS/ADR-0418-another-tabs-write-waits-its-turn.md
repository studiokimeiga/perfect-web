# ADR-0418: another tab's write waits its turn

Status: accepted 2026-11-10, landed with train-17. Proposed 2026-11-09, on
branch `perf/after-train-20261109`, under the owner's delegation of 2026-10-02
and order of 2026-10-25 ("deciding by best judgement without asking"), and the
order of 2026-11-09 to work the measured gaps with Svelte largest first.
Milestone: E14. Builds on ADR-0366's decision to show what another tab writes.

## Context

The head-to-head at `3b0e7c4f` has Svelte ahead on a press with 19 other tabs of
the app open: 44 ms against 29.8. A page that keeps its values on the device
hears each write another tab makes and draws it. With many tabs open, each write
in the tab someone is using sets all the others drawing at once, on the same
machine, while the tab in front is trying to make its frame.

The event also carries the value as it was when it was written, so a tab behind
on a burst of writes draws each in turn, and all but the last are already old.

## Decision

1. **A tab waits 50 ms before showing what it heard** (`heardLater`,
   `HEARD_AFTER`). That is three frames, too little to see in a tab no one is
   pressing in, and it lets the frame of the press go first.
2. **A burst shows once.** What is heard while a tab waits is kept by key, so
   several writes to one value are one render.
3. **What is shown is what the device holds when the tab looks**, not what the
   event carried (`heardValue(held, store.getItem(e.key))`). A later write
   cannot be overwritten by an earlier event.

A `clear()`, whose event has no key, waits the same way and reads each value.

## Why not the other ways

- **`requestIdleCallback`.** It was tried first and did not help: the machine
  was idle enough for it to fire while the press's frame was being made.
- **Only the visible tab draws**, and the others on becoming visible. A window
  beside another is visible and not focused; and a hidden tab's document would
  be out of date for its own next read.
- **A longer wait.** A person with two windows side by side sees the other one
  follow, and past about 100 ms it reads as lag.

## Result

Locally with 20 tabs, the press in the front tab takes 17.6 ms where it took
28.7. `e2e/device-store.spec.mjs` holds the burst and the later value, and
`scripts/device_store_mutations.py` removes each piece.

The head-to-head at `2d2b57ea`, a press in the last of twenty tabs of one
browser, the median of five runs in ms: TodoMVC's 29 (Svelte's 27, React's
27); the store's 21 (SvelteKit's 37, Next.js's 41).

## Consequences

- A value written in one tab is on screen in another 50 ms later than it was.
- A tab closed within those 50 ms draws nothing, which is what it would show.
