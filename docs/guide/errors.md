# Errors and diagnostics

The compiler's job is to refuse wrong programs, so you will read its
messages often. This page shows how to read one, and the mistakes a
TypeScript or React developer makes first, each with what `pw check`
prints for it. The output on this page is checked too: a test runs each
example and compares what the compiler prints with what the page shows.

## Reading a diagnostic

```pw fails=PW0606
module price

fn price(cents: Int) -> String !{} {
    cents
}
```

```text pw-check
error: [PW0606] `price.price` declares its result `String` and this produces `Int`
 --> price.pw:4:5
  |
3 | fn price(cents: Int) -> String !{} {
  |    ----- `price` declares its result here
4 |     cents
  |     ^^^^^ `price.price` declares its result `String` and this produces `Int`
  |
  = note: every value a body returns — its last expression, each branch of it, and each `return` — must have the result type its signature declares
  = help: produce a `String` here, or change the declared result
```

- `error: [PW0606]`: the code. [The codes](diagnostics.md) lists every one,
  with the rule it holds the program to.
- `--> price.pw:4:5`: the file, line and column.
- The `^^^^^` underline is where the rule is broken; a `-----` line points
  at what it was broken against.
- `= note` says why the rule exists; `= help` says how to repair it.

`pw check` exits 1 when it reports an error. `pw explain` prints each
declaration's type, effects, privacy and placement, as the compiler
derived them.

## Mistakes a TypeScript developer makes first

### `&&` and `||`

Pleris writes `&` and `|`:

```pw fails=PW0009
module logic

fn both(open: Bool, staffed: Bool) -> Bool !{} {
    open && staffed
}
```

```text pw-check
error: [PW0009] expected an expression, found `&`
 --> logic.pw:4:11
  |
4 |     open && staffed
  |           ^ expected an expression, found `&`
  |
  = help: Pleris writes `&` for and and `|` for or; write one operator
```

The help names the spelling: write `open & staffed`. The same rule
applies to `|` for or.

### A module's functions, used without importing it

The type `List<String>` needs no import, but its functions do:

```pw fails=PW0021
module count

fn count(names: List<String>) -> Int !{} {
    List.length(names)
}
```

```text pw-check
error: [PW0021] `List.length` does not resolve
 --> count.pw:4:5
  |
3 | / fn count(names: List<String>) -> Int !{} {
4 | |     List.length(names)
  | |     ^^^^^^^^^^^^^^^^^^ `List.length` does not resolve
5 | | }
  | |_- used inside `count`
  |
  = note: `List` is not a module this file can see. Names are visible through lexical scope, module membership, or an explicit import — external implementation is allowed, a missing declaration is not.
  = help: add `import List`, or declare it
```

### Reading what a command returned

In React you might `await` a mutation and use its result. A Pleris handler
learns only whether the command committed; the new data reaches the page
through the query the command invalidates, so the page has one source of
truth:

```pw fails=PW0620
module answer

import context.{ current_session }
import capability.{ Session, SessionId }

opaque type InteractionId = String

type Post = Post { id: String, text: String }

type PostError =
    | Unavailable

fn publish(s: Session<SessionId>, text: String) -> Result<Post, PostError> !{ database.write<Post> }
    host "blog:data/posts#publish"

command post(text: String) -> Result<Post, PostError>
    idempotent_by InteractionId
{
    publish(current_session(), text)
}

page Write() {
    route "/write"

    signal last: String = ""

    view {
        <title>Write</title>
        <button type="button" on:press={() => match post("Hello") {
            Ok(p) => last = "sent",
            Err(_) => (),
        }}>Post</button>
    }
}
```

```text pw-check
error: [PW0620] `post` answers whether it committed, not its value: `p` is bound to nothing
  --> answer.pw:30:16
   |
30 |             Ok(p) => last = "sent",
   |                ^ `post` answers whether it committed, not its value: `p` is bound to nothing
   |
   = note: A command's caller is answered `Ok`, or an error the command declares (ADR-0157). The value it produced reaches the page from the query it invalidates, so the page has one source for what it shows: a value read from the answer would be a second, which the query's next change would contradict.
   = help: write `Ok(_)`, and read the value from the page's query
```

An arm that does nothing is `()`, the unit value.

### A command a page can send twice

A browser can deliver a request twice. A command a page sends says how a
second delivery is answered:

```pw fails=PW0338
module twice

import context.{ current_session }
import capability.{ Session, SessionId }

opaque type InteractionId = String

type Post = Post { id: String, text: String }

type PostError =
    | Unavailable

fn publish(s: Session<SessionId>, text: String) -> Result<Post, PostError> !{ database.write<Post> }
    host "blog:data/posts#publish"

command post(text: String) -> Result<Post, PostError> {
    publish(current_session(), text)
}

page Write() {
    route "/write"

    signal sent: Int = 0

    view {
        <title>Write</title>
        <button type="button" on:press={() => match post("Hello") {
            Ok(_) => sent = sent + 1,
            Err(_) => (),
        }}>Post</button>
    }
}
```

```text pw-check
error: [PW0338] `post` is called by a page's handler, and declares no `idempotent_by`
  --> twice.pw:16:1
   |
16 | / command post(text: String) -> Result<Post, PostError> {
17 | |     publish(current_session(), text)
18 | | }
   | |_^ `post` is called by a page's handler, and declares no `idempotent_by`
...
27 |           <button type="button" on:press={() => match post("Hello") {
   |                                                       ------------- a press sends it from here
   |
   = note: A browser's request can reach the server twice: a retrying proxy, a flaky network or a client's own retry sends it again after the first arrived (RFC 9110 lets a client retry only a request it knows to be idempotent). Each press carries an interaction, and `idempotent_by InteractionId` runs the command once for it (ADR-0121).
   = help: declare `idempotent_by InteractionId`
```

`InteractionId` is a type the program declares, as this one does at its
top: `opaque type InteractionId = String`. Each press carries one, and
the command runs once for it.

### A button its reader may not press

A command that `requires SignedIn` is refused for a signed-out reader. A
page must not show its button to one:

```pw fails=PW5048
module guard

import context.{ current_session }
import capability.{ Session, SessionId }

opaque type InteractionId = String

type Post = Post { id: String, text: String }

type PostError =
    | Unavailable

predicate SignedIn
    says "Sign in to post."

fn publish(s: Session<SessionId>, text: String) -> Result<Post, PostError> !{ database.write<Post> }
    host "blog:data/posts#publish"

command post(text: String) -> Result<Post, PostError>
    requires      SignedIn
    idempotent_by InteractionId
{
    publish(current_session(), text)
}

page Write() {
    route "/write"

    signal sent: Int = 0

    view {
        <title>Write</title>
        <button type="button" on:press={() => match post("Hello") {
            Ok(_) => sent = sent + 1,
            Err(_) => (),
        }}>Post</button>
    }
}
```

```text pw-check
error: [PW5048] this control sends `post`, which `requires SignedIn`, and is shown where the page does not ask `SignedIn`
  --> guard.pw:33:9
   |
26 | /  page Write() {
27 | |      route "/write"
28 | |
29 | |      signal sent: Int = 0
...  |
33 | |/         <button type="button" on:press={() => match post("Hello") {
34 | ||             Ok(_) => sent = sent + 1,
35 | ||             Err(_) => (),
36 | ||         }}>Post</button>
   | ||________________________^ this control sends `post`, which `requires SignedIn`, and is shown where the page does not ask `SignedIn`
37 | |      }
38 | |  }
   | |__- `Write`
   |
   = note: A press the server refuses was a dead control: the owner, signed out, pressed Post and the post went with no word (2026-10-08). A page asks a predicate of its reader by naming it, and the deployment answers it as it answers `requires`, so a control shown under it is refused only if the reader changed since the page was rendered, and then told where the press was (ADR-0302). `SignedIn` is the program's to declare (`predicate SignedIn says "…"`) where only the deployment knows it.
   = help: show it where `SignedIn` holds: `{#if SignedIn}` around it, or `hidden={!SignedIn}` on it; where no reader's answer can decide it, as in a part served to everyone, mark its event `|refusable`
```

Wrapping the button in `{#if SignedIn} .. {/if}` repairs it, and the page
must then be a `session page`: a page that asks something of its reader is
not the same for every reader.

## When the build refuses what the check passed

`pw build` has refusals of its own. It prints them as lines that begin
`pw:`, and they carry no code. A program can check clean and be refused
there, so build before you trust a green check. The last section of
[part 3](tour-pages.md#where-pw-check-stops) shows what `pw check` passes
and `pw build` refuses, in programs the guide's test compiles.

## When the browser refuses a handler

A page's browser asks one question before it attaches a handler: may this
build's code read what this document captured? The decision answers it in
the browser, against the build that served the page, and a handler it
refuses is never downloaded. The usual cause is a document from another
build, which a cache kept or a redeploy outdated.

A refused button recovers when it is pressed:
- it reads the page again, never replaying the press;
- it asks first, where what the reader typed would be lost;
- where nothing safe is possible, it says the page is out of date.

The browser's console names the refusal by a code, and the code's page says
what it means:

```text
Pleris: handler 5 not attached: PWR04, https://pleris.dev/e/PWR04, recovery reload
```

| Code | Refusal | What it means |
|---|---|---|
| PWR02 | Hash scheme | The document's identities came from a hash scheme this build does not implement, so they cannot be compared at all. |
| PWR03 | Runtime ABI | The document was written for a runtime ABI this build does not support. |
| PWR04 | Unknown handler | This build has no code with the handler's identity. The page came from another build, or the handler changed since. |
| PWR05 | Privacy widened | The document's captures are scoped more narrowly than this page: a session's may not be resumed into a public page, nor one reader's into another's. |
| PWR06 | Document schema | The part of the document the handler belongs to has another shape in this build. |
| PWR07 | No migration | The handler reads another capture schema than the document holds, and the program declares no migration between them. |
| PWR08 | Migration failed | The declared migration between the two capture schemas failed on these captures. |
| PWR09 | Malformed | The document declares captures for the handler and carries none. |
| PWR10 | Mixed build | A streamed patch came from another build than the document. |

A document that carries no resume manifest gives the decision nothing to
read. The console then says `no decision` instead of a code, and the handler
stays unattached. That is the closed side: a page that does nothing, never
one that attaches anyway.

## Every code

[The diagnostic codes](diagnostics.md) lists all of them, generated from the
compiler's registry.
