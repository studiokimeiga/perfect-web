# Why Pleris

Pleris is a language for web apps, and a compiler and a server for it. You
write the data, the server's reads and writes, and the pages in one
language. The compiler then refuses programs with whole classes of bugs that
TypeScript, React, Next.js and Svelte let through to production.

This page shows what it refuses, with the decision records (ADRs) behind
each refusal, and then what it does not do yet. Every example on it is
compiled by a test, so what it says the compiler does is what it does.

## What the compiler refuses

### A user's data in a cache everyone shares

In Next.js, one wrong `revalidate` or a missing `cookies()` call can cache
one user's cart and serve it to the next visitor. In Pleris a query says
whose data it returns (`public`, `session` or `user`), and a session's data
cannot sit in a shared cache:

```pw fails=PW5001
module cart

import capability.{ Session, SessionId }

type Cart = Cart { items: Int }

fn cart_of(s: Session<SessionId>) -> Cart !{ database.read<Cart> }
    host "shop:data/carts#get"

session query MyCart(session: Session<SessionId>) -> Cart
    freshness     0.seconds
    consistency   read_your_writes
    cache         shared
    key           session
    concurrency   one_per_key
    on_key_change cancel
    timeout       2.seconds
{
    cart_of(session)
}
```

The label follows the value through calls, branches and returns (ADR-0128,
ADR-0129, ADR-0252, ADR-0282).

### Rendering that reaches the database

A React component can fetch in its body, in an effect, or in a helper three
calls down. In Pleris a `view` declares an empty effect row, `!{}`, and the
compiler checks the claim through every call it makes:

```pw fails=PW0401
module menu

type Item = Item { name: String }

fn items() -> List<Item> !{ database.read<Item> }
    host "shop:data/items#all"

view Menu() !{} {
    let all = items()
    <ul>
        {#each all as item (item.name)}
            <li>{item.name}</li>
        {/each}
    </ul>
}
```

A page reads data only through a declared `query`, which has a key, a cache
policy and a timeout (charter §7.4, §7.5). The [tour](tour.md) shows how.
One gap: the check does not look in an `{#each}` head ([where `pw check`
stops](tour-pages.md#where-pw-check-stops)).

### A function that says it is pure and is not

An effect row is a claim the compiler holds a function to. A helper that
reads the clock makes every caller that claims purity wrong:

```pw fails=PW0400
module stamp

import clock

fn stamp() -> Int {
    clock.now()
}

fn label() -> Int !{} {
    stamp()
}
```

### A case forgotten

A sum type's cases are checked in every `match`, as in Rust or Elm, and a
misspelt case is a case the type lacks, never a catch-all binding:

```pw fails=PW0305
module shapes

type Shape =
    | Circle(Int)
    | Square(Int)
    | Point

fn area(s: Shape) -> Int !{} {
    match s {
        Circle(r) => 3 * r * r,
        Square(side) => side * side,
    }
}
```

### A link to nowhere

Every internal link is matched against the program's routes (ADR-0160):

```pw fails=PW5009
module site

page Home() {
    route "/"

    view {
        <title>Home</title>
        <main>
            <a href="/abuot">About us</a>
        </main>
    }
}

page About() {
    route "/about"

    view {
        <title>About</title>
        <main>
            <h1>About us</h1>
        </main>
    }
}
```

### A form control nobody can name

A control a screen reader cannot announce is refused (ADR-0141), as are
invalid nesting, a duplicate `id` and a page without a `<title>`:

```pw fails=PW5014
module search

page Search() {
    route "/search"

    view {
        <title>Search</title>
        <main>
            <input type="text">
        </main>
    }
}
```

### And more, each with a code

A button that sends a command its reader may not run (ADR-0307), a command
a page can send twice without saying how a repeat is answered (ADR-0154), a
query that keeps running for a key the page no longer shows, an unchecked
cast of outside data, a secret serialized to the browser. The
[diagnostic codes](diagnostics.md) list all of them, from the compiler's
own registry.

## What it does not do yet

Pleris is a research project, and these are true today (2026-10-15):

- **Your app's tables live in memory.** A program declares its data as
  tables, and the development server serves every program's with one
  generic adapter, as it serves the [tutorial](tutorial.md)'s guestbook
  (ADR-0338): its rows are gone when the server stops, until a database
  behind the same declarations (PostgreSQL first) is built. A table has no
  owner yet: who may read or write a row is the program's own code (a
  command comparing a row's author with its reader). The reference apps'
  feed and store still read and write through data layers written in Rust
  (`host "feed:data/posts#timeline"`, ADR-0218), until their data is
  declared too.
- **What a predicate means is the server's.** A command `requires SignedIn`
  or `OwnsPost(post)`, and the development server knows three predicates by
  name (ADR-0302).
- **Sign-in is a development provider** on this machine (ADR-0258). There is
  no adapter for a real OpenID Connect provider yet.
- **One server.** A second server on the same database hears nothing of the
  first's writes (ADR-0246).
- **No project tooling.** There is no `pw new`, no project file and no
  editor support yet: you name the standard packages on the command line.
- **Gaps in the checks.** `pw check` is not the whole build: `pw build`
  refuses programs it passes (a `<title>` that reads a signal, a computed
  list in an `{#each}`), so build
  before you trust a green check. Effects in an `{#each}` head are now
  checked, including its list and key (ADR-0345). An `<img>` without `alt`
  is not refused. [The tour's last section](tour-pages.md#where-pw-check-stops)
  shows the remaining gaps and the repaired effect refusal in programs
  the guide's test compiles. The message for `&&` now names Pleris's `&`
  spelling ([errors](errors.md)).
- **Missing features**: offline and real-time collaboration, i18n,
  background jobs beyond a scheduled command (ADR-0334), and charts.
  `docs/vision/capabilities.md` keeps the full matrix.

## Where to go next

- The [language tour](tour.md): the language, one topic at a time.
- The [tutorial](tutorial.md): a guestbook with sign-in, built end to end
  and run on the development server.
- The [diagnostic codes](diagnostics.md).
