# The Pleris guide

Pleris is a language for web apps, with a compiler that refuses whole
classes of bugs before they ship. This guide is for a developer who knows
TypeScript and React, Next.js or Svelte, and for an AI agent given these
pages to write a Pleris app.

Read it in this order:

1. [Why Pleris](why.md): what the compiler refuses that other frameworks
   let through, and what Pleris does not do yet.
2. A tour of the language, in three parts:
   - [the language](tour.md): modules, types, records, sum types,
     functions, effects, results and money;
   - [data on the server](tour-server.md): host bindings, tables, sources,
     queries and live updates, commands, predicates, privacy and time;
   - [pages](tour-pages.md): routes and query parameters, markup, signals
     and handlers, forms, layouts and navigation.
3. [Errors and diagnostics](errors.md): reading what the compiler says,
   and the first mistakes a TypeScript developer makes.
4. [Tutorial: a guestbook](tutorial.md): one small app with sign-in, built
   end to end and run on the development server.
5. [The diagnostic codes](diagnostics.md): every code, its rule and its
   ADR.

A word you do not know, in these pages or in the ADRs (*track*, *evidence*,
*set read*, *the seam*)? [The glossary](../GLOSSARY.md) maps the project's
terms to standard ones and says where each is defined.

## Every example compiles

Each Pleris example in this guide is a whole program, and a test,
`compiler/pw-cli/tests/guide.rs`, runs every one through the real compiler
with the standard packages:

- an example meant to compile must check with no diagnostic, be formatted
  as `pw fmt` writes it, and build with `pw build`;
- an example meant to fail must fail with exactly the codes it names, and
  where the page shows the compiler's output, the compiler must print
  exactly that;
- an example that shows a gap in the checks (`pw check` passes a program
  `pw build` refuses, or one the page says it should refuse) must still be
  one: when the compiler closes the gap, the test fails and the page is
  updated;
- the tutorial's code, reassembled from its page, must be byte for byte
  the app in `examples/guestbook/`, which a browser test drives in
  Chromium, Firefox and WebKit;
- the list of codes is generated from the compiler's registry.

So `just ci` fails when the language changes under an example. The design
is ADR-0343, "a guide whose examples compile"
(`docs/DECISIONS/ADR-0343-a-guide-whose-examples-compile.md`).

## What the guide does not cover yet

Streamed regions (`<stream>`), materializations, uploads, payments,
resources and tasks, the frame phases of layout code, and WIT components
are in the ADRs and the reference apps (`examples/feed`, `examples/store`,
`examples/kiokun-site`), not here yet.

There is no `pw new` and no editor support yet: you name the standard
packages on the command line, as the tutorial shows.
