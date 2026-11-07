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
    requires      Anyone
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
  --> answer.pw:31:16
   |
31 |             Ok(p) => last = "sent",
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

type Post = Post { id: String, text: String }

type PostError =
    | Unavailable

fn publish(s: Session<SessionId>, text: String) -> Result<Post, PostError> !{ database.write<Post> }
    host "blog:data/posts#publish"

command post(text: String) -> Result<Post, PostError>
    requires Anyone
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
error: [PW0338] `post` is called by a page's handler, and declares no `idempotent_by`
  --> twice.pw:14:1
   |
14 | / command post(text: String) -> Result<Post, PostError>
15 | |     requires Anyone
16 | | {
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

`InteractionId` is a type your program declares:
`opaque type InteractionId = String`.

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

### A command anyone can run, by saying nothing

No page sends `delete_post`, and that protects nothing: the server runs a
command for any request that names it. So every command says who may:

```pw fails=PW0367
module admin

opaque type PostId = String

type Post = Post { id: PostId, text: String }

type PostError =
    | Missing

fn remove(post: PostId) -> Result<(), PostError> !{ database.write<Post> }
    host "blog:data/posts#remove"

command delete_post(post: PostId) -> Result<(), PostError> {
    remove(post)
}
```

```text pw-check
error: [PW0367] `delete_post` runs for any reader's request, and says nothing of who may run it
  --> admin.pw:13:9
   |
13 |   command delete_post(post: PostId) -> Result<(), PostError> {
   |   -       ^^^^^^^^^^^ `delete_post` runs for any reader's request, and says nothing of who may run it
   |  _|
   | |
14 | |     remove(post)
15 | | }
   | |_- a request that names `delete_post` runs it, with or without a page
   |
   = note: The server runs a command for any request that names it, whether or not a page shows its button: a request is an address and a body, and anyone can send one. So who may run a command is the command's to say. `requires` names what every request must hold, as `requires SignedIn, OwnsPost(post)`; `requires Anyone` says anyone may, and stands alone. A command the platform runs says `invoked_by` instead.
   = help: declare `requires` with what a reader must hold, such as `requires SignedIn`
   = help: or `requires Anyone`, if anyone may run it
```

`requires Anyone` is how a command says anyone may run it, where a reviewer
reads it.

### A row anyone can change, by naming it

`forget_note` says who may run it, and nothing of whose note it forgets:
sent with another reader's id, it deletes theirs. So a table that a
reader's command writes says whose its rows are:

```pw fails=PW0370
module Notes

opaque type NoteId = String

type Note = Note { id: NoteId, text: String }

table Notes
    row Note
    key id

command forget_note(note: NoteId) -> ()
    requires Anyone
{
    Notes.delete(note)
}
```

```text pw-check
error: [PW0370] `Notes` is written by a command any reader's request runs, and says nothing of who may change its rows
  --> Notes.pw:7:7
   |
 7 | table Notes
   |       ^^^^^ `Notes` is written by a command any reader's request runs, and says nothing of who may change its rows
...
14 |     Notes.delete(note)
   |     ------------------ `forget_note` writes it, and runs for any reader's request
   |
   = note: A command a reader can run is a request away, and its arguments are the request's: `drop_item(list, item)` deletes whichever item a request names, anyone's. So a table such a command writes says whose its rows are. With `owner user` or `owner session` the platform records the reader who inserts each row, in no field the program sees, and a `put`, a `delete` or a `delete_of_<fields>` from a request changes only its reader's rows: another reader's row is treated as absent. `owner anyone` says any reader may change any row, outright. A command the clock runs for a reader's schedule writes for that reader; a table only the platform's other commands (`invoked_by`) write needs none.
   = help: declare `owner user` or `owner session`: the platform records the reader who inserts each row, and only that reader's requests change it
   = help: or `owner anyone`, if any reader may change any row
```

`owner session` repairs it: the platform records the session that inserts
each note, and a request deletes its own notes alone. `owner anyone` says
that any reader may change any note, where a reviewer reads it.

## When the build refuses what the check passed

`pw build` has refusals of its own. It prints them as lines that begin
`pw:`, and they carry no code. A program can check clean and be refused
there, so build before you trust a green check. The last section of
[part 3](tour-pages.md#where-pw-check-stops) shows what `pw check` passes
and `pw build` refuses, in programs the guide's test compiles.

## Every code

[The diagnostic codes](diagnostics.md) lists all of them, generated from the
compiler's registry.
