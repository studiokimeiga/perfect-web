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

```text pw-check
error: [PW5001] `MyCart` is Session<SessionId> and declares a shared cache
  --> cart.pw:13:5
   |
10 | session query MyCart(session: Session<SessionId>) -> Cart
   |                      --------------------------- `session` is Session<SessionId>, so what `MyCart` makes from it is
...
13 |     cache         shared
   |     ^^^^^^^^^^^^^^^^^^^^ `MyCart` is Session<SessionId> and declares a shared cache
   |
   = note: A shared cache may contain only `Public` values, because every user reads it. The cause chain: `MyCart` materializes its parameter `session`, which is labeled `Session<SessionId>`, and `Session<SessionId>` does not flow into `Public` — so one user's value would be served to another. A key naming the reader does not make it shareable: an entry only that reader can hit gains nothing from a shared cache, and puts their value where every reader's is held (ADR-0128).
   = help: make the cache private, which stores per session
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

### A secret rendered into the page

A Next.js server component can hand any value to a client component, and
React writes it into the page every visitor of it downloads. In Pleris a
secret is labelled where it is made, and the label follows it to the page:

```pw fails=PW5003
module checkout

import secrets
import capability.{ Secret, Payments }

page Checkout() {
    route     "/checkout"
    placement origin

    let key: Secret<Payments> = secrets.payments()

    view {
        <title>Checkout</title>
        <pay-button api_key={key} />
    }
}
```

```text pw-check
error: [PW5003] `key` is Secret<Payments> and is rendered into markup
  --> checkout.pw:14:30
   |
10 |     let key: Secret<Payments> = secrets.payments()
   |     ---------------------------------------------- `key` becomes Secret<Payments> here
...
14 |         <pay-button api_key={key} />
   |                              ^^^ `key` is Secret<Payments> and is rendered into markup
   |
   = note: Markup is sent to the browser. Secret<Payments> may exist only in the origin world, so rendering it moves it across a boundary it may not cross — the value is in the HTML whether or not anything reads it.
   = help: perform the privileged operation on the origin and render only its result
```

### An old answer shown for a new question

A search box that fetches as its reader types can show the answer to an
earlier word, when that answer arrives last. A query whose key can change
says what happens to the work for the old key:

```pw fails=PW0325
module search

type Item = Item { name: String }

fn matching(q: String) -> List<Item> !{ database.read<Item> }
    host "shop:data/items#search"

public query Search(q: String) -> List<Item>
    freshness   30.seconds
    consistency snapshot
    cache       shared
    concurrency one_per_key
    timeout     2.seconds
{
    matching(q)
}
```

```text pw-check
error: [PW0325] `Search` declares no policy for superseded keys
  --> search.pw:12:5
   |
 8 | public query Search(q: String) -> List<Item>
   |              ------ this query is keyed, so its key can change
...
12 |     concurrency one_per_key
   |     ^^^^^^^^^^^^^^^^^^^^^^^ `Search` declares no policy for superseded keys
   |
   = note: when a keyed read's key changes while it runs, the work for the old key is stale; a policy says whether it is cancelled or kept, and without one the old answer can arrive after the new key and be shown for it
   = help: declare `on_key_change cancel | supersede | keep`
```

### And more, each with a code

A button that sends a command its reader may not run (ADR-0307), a command
a page can send twice without saying how a repeat is answered (ADR-0154),
both shown in [errors](errors.md); an unchecked cast of outside data; a
failure left unhandled. The [diagnostic codes](diagnostics.md) list all of
them, from the compiler's own registry.

## What it does not do yet

Pleris is a research project, and these are true today (2026-10-15):

- **Your app's storage is Rust.** A program reads and writes through *host
  bindings*, functions whose body is an operation the server supplies
  (`host "feed:data/posts#timeline"`, ADR-0218, ADR-0262). The development
  server has three data layers, written in Rust for the reference apps: a
  social feed's, a food-delivery store's and a dictionary's. A new app uses
  one of them, as the [tutorial](tutorial.md)'s guestbook uses the feed's,
  or someone writes a new one in Rust. A program cannot yet declare its own
  storage.
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
