# TodoMVC's official React and Svelte apps: what is vendored here, and from where

The head-to-head (ADR-XXXX, "the head-to-head") measures TodoMVC in Pleris
(`examples/todomvc`) against TodoMVC's **own** React and Svelte
implementations, never rewritten here (ADR-0342 §4: "taken from TodoMVC's own
repository and never written here"). These two directories are those
implementations, byte for byte, as their repository ships them, committed
production builds included.

- **Source**: <https://github.com/tastejs/todomvc>, `master` at
  `ff43b02e59dfa604386bb382034b2cd07c2bcd8a` ("Revise README for 2.0.0",
  2026-05-03), the commit TodoMVC's Cypress suite was vendored from
  (`spikes/todomvc-cypress/UPSTREAM.md`). `master` was still at it on
  2026-10-25, when these were fetched (GitHub's commits API; the last commit
  touching either directory is `1f2bd7f0`, "Refresh of the modern framework
  examples (#2297)", 2026-05-03).
- **How**: `git fetch --depth 1 --filter=blob:none` of that commit and a sparse
  checkout of `examples/react/`, `examples/svelte/` and `license.md`, into an
  empty directory of its own (2026-10-25). Git checks each object against its
  name; each file below is that blob (`git hash-object <file>`), the same as
  the tree GitHub's API lists for the commit.
- **Licence**: MIT, "Copyright (c) Addy Osmani, Sindre Sorhus, Pascal Hartig,
  Stephen Sawchuk" (`LICENSE-todomvc.md`, the repository's `license.md`:
  "Everything in this repo is MIT License unless otherwise specified"; neither
  example's `package.json` names another licence). The builds bundle
  third-party code under its own licences, which travel with them: React,
  ReactDOM, the JSX runtime and the scheduler (MIT, Meta), classnames (MIT,
  Jed Watson) and react-router (MIT), as `react/dist/app.bundle.js.LICENSE.txt`
  lists; Svelte's runtime (MIT); `todomvc-common`'s `base.js` (MIT); and in
  both stylesheets `todomvc-app-css` 2.4.3, **CC BY 4.0**
  (`LICENSE-todomvc-app-css.txt`, copied from the package the owner approved
  on 2026-10-08, as `spikes/own-renderer/todomvc.sh` carries it).
- **Approved**: the owner's standing order of 2026-10-25 ("downloads of
  well-maintained, permissively licensed dependencies that a track genuinely
  needs (e.g. ... the head-to-head's official TodoMVC apps) proceed on
  judgement: record source, version, licence and size"), after the request
  recorded in ADR-0342 ("Downloads the owner is asked about").
- **Size**: upstream's `examples/react` is 26 files, 2,011,587 bytes (its
  `dist/` 1,625,927, of which source maps 1,375,884); `examples/svelte` is 17
  files, 123,152 bytes (its `dist/` 57,345). Vendored here: react 24 files,
  635,703 bytes; svelte 17 files, 123,152 bytes.

## Not vendored

| upstream path | git blob | bytes | why |
|---|---|---|---|
| `examples/react/dist/app.bundle.js.map` | `a43e8d03149794811e95c19e21e6f3ba20cf561b` | 1,361,509 | a source map: a browser fetches it only for its developer tools, so no measurement reads it |
| `examples/react/dist/app.css.map` | `051cd5bf94db5ccb93c72447a99a364ccb2905f7` | 14,375 | the same |

`react/dist/app.bundle.js` still ends with its `sourceMappingURL` comment, as
shipped.

## Vendored

| file here | git blob | bytes |
|---|---|---|
| `LICENSE-todomvc.md` (`license.md`) | `9275d4371020294b6a107e454a0d1819f8b71c31` | 1,166 |
| `react/.eslintrc` | `0ce034b427c21bea76786762e36423f7170f1069` | 277 |
| `react/.gitignore` | `03e05e4c07174bdbd4a86a6206af4fa88b03f743` | 24 |
| `react/dist/app.bundle.js` | `08741691a2f21c53f816811aa5750f7448b1f376` | 236,914 |
| `react/dist/app.bundle.js.LICENSE.txt` | `8176893c2c9fd488645d2fd92ca4119d0e49f693` | 1,332 |
| `react/dist/app.css` | `676204ebeac3d26572e969bdc229dbbce7237336` | 7,400 |
| `react/dist/base.js` | `013aacecb4f9f2e87b04552cd8b571d53cf55c0d` | 3,752 |
| `react/dist/index.html` | `82aa905b19394a7537790670445e3848d1d02c47` | 645 |
| `react/package-lock.json` | `979346e947c1cedebb90a6acb1b00c120bae6c97` | 367,733 |
| `react/package.json` | `46c9ec6649f1d063d6512009c24f96f801d8e844` | 1,236 |
| `react/public/index.html` | `2bbfd1b8f22739ff97b7a8f2247dae6363d279c2` | 682 |
| `react/readme.md` | `1f3ce1e48c8ae457d89e1642ba4791d7dea56bca` | 1,218 |
| `react/src/index.js` | `b157dc88316fa6a11c283ff7bb0320341d67c4c0` | 389 |
| `react/src/todo/app.css` | `bfa6617eb55ab28c3a0790065b69921334ccf7cc` | 477 |
| `react/src/todo/app.jsx` | `4ae2b60d3191b1c601fb17a7f5fb801d7e47ec93` | 518 |
| `react/src/todo/components/footer.jsx` | `70b7ac516169d7e770c7ceb77711df1ed39eeb53` | 1,544 |
| `react/src/todo/components/header.jsx` | `866d2bf9c064e46124aace65e5febc1c51ac566c` | 484 |
| `react/src/todo/components/input.jsx` | `bfaab32bce31b1513d355295a110238caecd1ac8` | 1,293 |
| `react/src/todo/components/item.jsx` | `e558306aa65306f155c83a9e2f455e034aac7cb9` | 2,279 |
| `react/src/todo/components/main.jsx` | `4cf1a8abe37655774e2a06fcb509fd236d5f9400` | 1,617 |
| `react/src/todo/constants.js` | `cb019df4c3f8310827d4e45bd7c14c069bb5fee0` | 318 |
| `react/src/todo/reducer.js` | `b43c3583fd9346332726ad37984c5d79bd57770e` | 3,012 |
| `react/webpack.common.js` | `ed3adfe93beb7abaa4e475408d3b5932d3cc82be` | 1,182 |
| `react/webpack.dev.js` | `774d05584d682f907015761eecb8dcee0c1617df` | 402 |
| `react/webpack.prod.js` | `ef4bbc0d13c2d070f68191c4bfb07ad1f63c1cbf` | 975 |
| `svelte/.gitignore` | `4992926ba8b5dbf6165c7753b3af7e68540d1275` | 134 |
| `svelte/dist/assets/index-BxPr2-fK.js` | `a909bfc0fb055d1dffda0b168e0536c7a87dc0c4` | 42,122 |
| `svelte/dist/assets/index-bgjkvPzV.css` | `59c89c15fbd58f5aadd1bd2a658e8ebaf30ff88a` | 7,129 |
| `svelte/dist/base.js` | `a56b5aaca980fb608faf3eb9624750c8740b0025` | 7,253 |
| `svelte/dist/index.html` | `8ea1fde690a0198d444efc10c1c3c4b7f747cc6a` | 841 |
| `svelte/index.html` | `0c08783c616c9312008e3aaf464b3fd78d874963` | 793 |
| `svelte/package-lock.json` | `1288f99320ef27fe303784c3629d57c73f241dfe` | 55,830 |
| `svelte/package.json` | `a2a7498d421c14dcc228ccfec7ef777761140342` | 657 |
| `svelte/readme.md` | `0911855422748cc30448fefac96626c790744a8a` | 1,990 |
| `svelte/src/App.svelte` | `46c416d53f52077038b6cd83d83c56bdd1c8a2b2` | 2,137 |
| `svelte/src/Footer.svelte` | `b73039a2ecd26a52760a1160aa3285d51af15c6e` | 738 |
| `svelte/src/Header.svelte` | `89e5aeab694af9601db7f9a5528b77486dfc05bd` | 456 |
| `svelte/src/Item.svelte` | `6216339b7635b7b38815b09da37c62684e28ea24` | 1,407 |
| `svelte/src/app.css` | `bfa6617eb55ab28c3a0790065b69921334ccf7cc` | 477 |
| `svelte/src/index.js` | `7c97217404dfc0e3b1300644c9c15e7ce8fcd01b` | 163 |
| `svelte/src/router.js` | `65a9c65591f9aff265e801a677da8ef9c0d903bc` | 528 |
| `svelte/vite.config.js` | `845174f2982d50c13bec7028a5e9cd2e28103f72` | 497 |

`LICENSE-todomvc-app-css.txt` is `todomvc-app-css` 2.4.3's `license`
(blob `2d6ec366a6da8fa4c9208d8b798219c6639180b8`, 18,752 bytes), from the
package installed for `spikes/own-renderer`.

`upstream.json` is this table as data, from the fetched checkout. The
head-to-head (`../validate.mjs`, its build rule) holds every file listed there
to its blob, and every file a page is sent from these directories to the file
of the same name: it refuses to report a measurement of a vendored file that
is not upstream's, or of a served file that is not the vendored one.

## What the builds are

Each is the production build its repository commits, built by the example's
own toolchain, and measured as it is: neither is rebuilt here.

- **React**: webpack 5.106.2 with Babel (`@babel/preset-env` "defaults",
  `@babel/preset-react` automatic runtime), `mode: "production"`, Terser 5.46.2
  and CSS Minimizer (`webpack.prod.js`); React and ReactDOM 19.2.5,
  react-router-dom 7.14.2 (`HashRouter`), classnames 2.5.1, by
  `package-lock.json`. One script, `app.bundle.js`, not content-hashed.
- **Svelte**: Vite 8.0.10 with `@sveltejs/vite-plugin-svelte` 7.0.0 and Svelte
  5.55.5 (`package-lock.json`). One module script and one stylesheet, each
  named by its content's hash.
- Both ship `todomvc-common`'s `base.js` (React's minified by Terser), which
  TodoMVC's examples carry for their shared sidebar; TodoMVC in Pleris does
  not load it. The head-to-head reports bytes with and without TodoMVC's
  common files, for every stack alike.

## What the apps do and do not do

Both keep their todos in memory only: a reload empties the list. TodoMVC's
own Cypress suite knows this (its `noLocalStorageCheck` table names `react`
and `svelte`, and its persistence test skips itself for them). TodoMVC's
specification (`app-spec.md`, "Persistence") asks every app to keep its todos
in `localStorage`; the head-to-head reports this as each app's gap, and holds
TodoMVC in Pleris to it.
