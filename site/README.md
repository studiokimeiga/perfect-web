# pleris.dev

The project's site. `build.mjs` builds it into static HTML from the
repository:

- the home page;
- the guide (`docs/guide/*.md`);
- a page for every diagnostic code, from `docs/guide/diagnostics.md`, which
  `scripts/guide_diagnostics.py` writes from the compiler's registry.

Cloudflare serves the result as it is.

## Build and check

```sh
node site/build.mjs              # into site/dist
node --test site/build.test.mjs
```

The build has no dependency: `markdown.mjs` reads the Markdown the guide
writes. The tests hold three things:

- the reader, construct by construct;
- Pleris highlighting, which adds spans and changes no text;
- the whole built site:
  - every guide page and every code has a page;
  - no page has a script, or a style the policy did not hash;
  - no internal link or anchor lands nowhere.

## What it serves

- **Pages:**
  - `/`, the home page;
  - `/guide/…`, the guide, in its reading order;
  - `/e/<code>`, a page for each code, retired codes and old numbers
    included. Where the guide has an example the compiler refuses with
    that code, the page shows it with the compiler's output;
  - `/e/`, every code, with its rule.
- **Nothing else runs or loads:** no script, no cookie, no request to
  another origin.
- **Policy (`_headers`):** the one stylesheet is written into each page and
  allowed by its hash. HSTS is on, and no page may be framed.

## Deploy

The site runs on Cloudflare's Workers static assets:

- **`pleris.dev`** (`wrangler.jsonc`) serves `dist` as it is. No Worker
  script runs, so a request costs nothing.
- **`www.pleris.dev`** (`www/`) answers every request with a 301 to the
  same path on `pleris.dev`.

```sh
cd site && npx --yes wrangler@4.152.0 deploy
cd site/www && npx --yes wrangler@4.152.0 deploy
```

wrangler is pinned to 4.152.0, released 2026-10-22, with no patch release
since. Run `wrangler login` once, in a browser, before the first deploy.

## The runtime's codes

The browser runtime's refusal codes will get pages here too. Its log lines
will link to `pleris.dev/e/<code>`, from the change that gives them codes.

## Next: built with Pleris

This is the site where Pleris should prove itself, and it is not built with
Pleris yet. A Pleris app today runs on its own Rust server, which
Cloudflare's edge does not run, and Pleris has no static or edge build. The
site moves to Pleris when Pleris has:

1. **A static build:** `pw build` writing each known route's HTML (the
   guide's pages, the codes) at build time.
2. **Content Pleris can render:** the guide's Markdown as pages, or its
   content as Pleris data.
3. **A deploy target:** an edge or static host (Cloudflare's), or a hosted
   Pleris server.

Until then the content stays in the repository's Markdown. A Pleris build
can then take over this generator's job without moving any page.
