# Tutorial: a guestbook

<!-- guide-program: examples/guestbook -->

This tutorial builds a small app end to end: a guestbook. Anyone can read
it. A signed-in reader can sign it, and remove what they wrote. Every open
page shows a new entry, and a removed one goes, without a reload.

You write two files:

- `guestbook/book.pw`: the data, the reads and the writes;
- `guestbook/pages.pw`: the page.

The code below is the whole app, in order. A test reassembles it from this
page and holds it, byte for byte, to `examples/guestbook/`, which is what
the development server runs, and the browser test drives. So what you read
here is what runs.

## Before you start

You need the repository and its toolchain (`just doctor` says what is
missing). Build the compiler once:

```sh
cargo build -p pw-cli
```

The compiler is then `target/debug/pw`. There is no project file yet:
you name the standard packages on the command line each time.

**Where the data lives.** A Pleris program reads and writes through *host
bindings*: functions whose body is an operation the server supplies. Today
the development server supplies the operations of three data layers,
written in Rust for the reference apps, and has no way yet for a program
to declare its own storage. So the guestbook uses the data layer of the
reference app `examples/feed`, a small social feed: a guestbook entry is a
post in it. That is why some names below are the feed's (`Post`,
`feed:data/posts#timeline`).

## 1. A module and its imports

Every file is a module. It names the modules it uses: `List` and `String`
are the standard library's, and `context` and `capability` are the web
platform's.

```pw file=guestbook/book.pw
module guestbook.book

import List
import String
import context.{ current_session }
import capability.{ Session, SessionId, UserId }
```

`import List` gives the module, so you write `List.length(xs)`. The type
`List<T>` needs no import. `import context.{ current_session }` gives one
name, used bare.

## 2. The words of an entry

An entry's text is not any string. It is something, and at most 280
characters. Say so once, as a type:

```pw file=guestbook/book.pw
opaque type PostId = String

opaque type InteractionId = String

opaque type Words = String where String.length(value) >= 1 & String.length(value) <= 280

fn words(draft: String) -> Option<Words> !{} {
    let text = String.trim(draft)
    if String.length(text) >= 1 & String.length(text) <= 280 {
        Some(Words(text))
    } else {
        None
    }
}
```

- `opaque type PostId = String` is a new type whose values are strings
  inside. Outside its module nothing can mistake a `PostId` for any other
  string.
- `where ...` is the type's invariant. The compiler checks every place
  that makes a `Words` (`Words(text)`), and here it is shown to hold by
  the `if` around it. Construct one anywhere the condition is not known,
  and the compiler refuses it (PW0622).
- `words` returns `Option<Words>`: `Some(words)` or `None`. There is no
  `null`.
- `!{}` is the function's effect row, empty: the function is pure, and
  the compiler holds it to that.
- `InteractionId` is the type a command's repeated sends are told apart
  by (step 6).

## 3. What the server keeps

The records the data layer answers with, and what can go wrong:

```pw file=guestbook/book.pw
type User = User { id: UserId, name: String }

type Post = Post { id: PostId, author: User, text: String }

type Entry = Entry { id: PostId, author: User, text: String, mine: Bool }

type Viewer = Viewer { signed_in: Bool, name: String }
```

A record type names its fields. The data layer's records may have more
fields than these: a program receives the fields it declares, by name, and
no others (ADR-0291). `Entry` is a post as a list shows it, with `mine`
true where the reader wrote it. `Post` is what a write answers.

```pw file=guestbook/book.pw
type BookError =
    | Unavailable
    | NotFound
```

`BookError` is a *sum type*: a value is one of its cases. A `match` on it
must handle every case.

```pw file=guestbook/book.pw
event Signed(author: UserId)

event Removed(post: PostId)
```

Events are facts the app's commands announce: someone signed, an entry was
removed. Queries name the events that make them stale (step 5).

```pw file=guestbook/book.pw
source Guestbook
    holds        Post, User
    transactions serializable
    reads        strong
    changes      none
```

A `source` states what the database behind these types guarantees:
serializable transactions, reads of the latest commit, and no feed of
changes of its own. The compiler holds every query and command to it
(a query cannot ask for more than its source's reads give), and the server
compares it with the database it opens before serving anything
(ADR-0207, ADR-0246).

## 4. Reading and writing through the data layer

Each host binding is a function with no body. `host "..."` names the
operation the server supplies, and the effect row says what it does:

```pw file=guestbook/book.pw
fn entries(s: Session<SessionId>, limit: Int) -> List<Entry> !{ database.read<Post> }
    host "feed:data/posts#timeline"

fn publish(s: Session<SessionId>, text: Words) -> Result<Post, BookError> !{ database.write<Post> }
    host "feed:data/posts#publish"

fn delete(s: Session<SessionId>, post: PostId) -> Result<PostId, BookError> !{ database.write<Post> }
    host "feed:data/posts#delete"

fn author(s: Session<SessionId>) -> UserId !{ database.read<User> }
    host "feed:data/users#of-session"

fn viewer(s: Session<SessionId>) -> Viewer !{ session.read }
    host "feed:data/users#viewer"
```

- `database.read<Post>` and `database.write<Post>` are effects. A
  function that calls `entries` performs `database.read<Post>` too. If it
  writes an effect row, the row must name it (PW0400); if it writes none,
  the compiler works the effects out, and they pass on to its callers.
- `Session<SessionId>` is the reader's session, a value only the platform
  makes (`current_session()`). Anything read with it is private to that
  session.

## 5. Queries the page reads

A page never calls a host binding itself. It reads a `query`: a declared,
keyed read, with a freshness, a cache and a timeout.

```pw file=guestbook/book.pw
session query Me(session: Session<SessionId>) -> Viewer
    freshness     0.seconds
    consistency   read_your_writes
    cache         private
    key           session
    concurrency   one_per_key
    on_key_change cancel
    timeout       2.seconds
{
    viewer(session)
}
```

```pw file=guestbook/book.pw
session query Entries(session: Session<SessionId>, limit: Int) -> List<Entry>
    freshness      0.seconds
    consistency    read_your_writes
    cache          private
    key            session, limit
    invalidates_on Signed(_), Removed(_)
    concurrency    one_per_key
    on_key_change  cancel
    timeout        2.seconds
{
    entries(session, limit)
}
```

- `session query` says the result is the session's: it is cached privately,
  per session, and never in a cache another reader shares.
- `key session, limit`: each session and limit is one entry in the cache.
- `invalidates_on Signed(_), Removed(_)`: when a command announces either
  event, every open page that shows this query is read again and told the
  new value, live. This is how the guestbook updates without a reload.
- `on_key_change cancel`: when the page asks for another `limit`, the old
  read is cancelled rather than left to finish.

## 6. Commands that write

A `command` is the one way a page changes data. First, who may:

```pw file=guestbook/book.pw
predicate SignedIn
    says "Sign in to sign the guestbook."

predicate OwnsPost(post: PostId)
    says "Only its author can remove an entry."
```

A predicate's meaning is the server's: the development server knows
`SignedIn` (the session has signed in) and `OwnsPost(post)` (the reader
wrote the post). The program says, in its own words, what a reader is told
when one refuses.

```pw file=guestbook/book.pw
command sign(text: Words) -> Result<Post, BookError>
    requires      SignedIn
    idempotent_by InteractionId
    emits         Signed(author(current_session()))
    optimistic    Entries(current_session(), _) as shown => pending(shown, text)
{
    publish(current_session(), text)
}

fn pending(shown: List<Entry>, text: Words) -> List<Entry> !{} {
    let you = User { id: UserId("you"), name: "You" }
    let entry = Entry { id: PostId("pending-{List.length(shown)}"), author: you, text: text.value, mine: false }
    List.concat([entry], shown)
}
```

- `requires SignedIn`: checked by the server before the body runs. A
  refused press is answered 403, and the page says the predicate's words
  where the button was.
- `idempotent_by InteractionId`: a press's request sent again (a retry)
  commits once. A double click is two presses, and signs twice.
- `emits Signed(...)`: the event is written with the post, in the same
  transaction, so it is sent if and only if the post is kept.
- `optimistic Entries(...) as shown => pending(shown, text)`: the page
  shows the entry at once, before the server answers. `pending` is a pure
  function of the list the page holds. If the command fails, the platform
  puts back the list it held: there is no rollback to write.

The pending entry has an id the server never gave, and `mine: false`, so it
offers no Remove button until the server's row takes its place.

```pw file=guestbook/book.pw
command remove(post: PostId) -> Result<PostId, BookError>
    requires      SignedIn, OwnsPost(post)
    idempotent_by InteractionId
    emits         Removed(post)
{
    delete(current_session(), post)
}
```

`requires SignedIn, OwnsPost(post)`: only the author can remove an entry.

## 7. The page

The second file holds the page:

```pw file=guestbook/pages.pw
module guestbook.pages

import List
import String
import context.{ current_session }
import guestbook.book.{ Me, Entries, SignedIn, words, sign, remove }
```

```pw file=guestbook/pages.pw
session page Book() {
    route     "/"
    placement origin
    cache     private

    signal shown: Int = 20
    signal draft: String = ""
    let me = query Me(current_session())
    let entries = query Entries(current_session(), shown)

    view {
        <title>Guestbook</title>
        <main>
            <h1>Guestbook</h1>
            {#if me.signed_in}
                <p id="me">Signed in as {me.name}</p>
                <form method="post" action="/sign-out"><button type="submit">Sign out</button></form>
            {:else}
                <p id="signed-out">Sign in to sign the guestbook.</p>
                <form method="get" action="/sign-in"><button type="submit">Sign in</button></form>
                <form method="get" action="/sign-up"><button type="submit">Sign up</button></form>
            {/if}
            <form hidden={!SignedIn} on:submit|prevent={() => match words(draft) {
                Some(text) => match sign(text) {
                    Ok(_) => draft = "",
                    Err(_) => (),
                },
                None => (),
            }}>
                <label>Your message <textarea bind:value={draft}></textarea></label>
                <button type="submit" disabled={String.length(draft) == 0}>Sign</button>
            </form>
            {#if List.length(entries) == 0}<p id="empty">No one has signed yet.</p>{/if}
            <ul>
                {#each entries as entry (entry.id)}
                    <li>
                        <p>{entry.text}</p>
                        <p class="by">{entry.author.name}</p>
                        {#if SignedIn}{#if entry.mine}<button type="button" on:press={() => match remove(entry.id) {
                            Ok(_) => (),
                            Err(_) => (),
                        }}>Remove</button>{/if}{/if}
                    </li>
                {/each}
            </ul>
            <button type="button" on:press={() => shown = shown + 20}>Show more</button>
        </main>
    }
}
```

Read it from the top:

- `session page Book()` at `route "/"`: the page is the session's, rendered
  at the origin for each reader, and cached privately.
- `signal shown: Int = 20` and `signal draft: String = ""` are the page's
  own state, in the browser. A handler assigns them; the page shows them.
- `let me = query Me(current_session())` reads a query. The page renders on
  the server with its value, and the browser is told when it changes.
- `{#if me.signed_in} .. {:else} .. {/if}`, `{#each entries as entry
  (entry.id)} .. {/each}`: markup blocks, as in Svelte. A list is keyed, so
  each row keeps its place as rows come and go.
- `hidden={!SignedIn}` and `{#if SignedIn}`: the page asks the predicate
  of its reader, as the server will. A control that sends a command which
  `requires SignedIn` must be shown only where the page asks it: the
  compiler refuses it otherwise (PW5048), so a signed-out reader is never
  given a button that can only be refused.
- `on:submit|prevent={() => ...}`: the handler, run in the browser. It
  makes `Words` from the draft, calls the command, and matches on the
  answer, which says only whether the command committed: the new entry
  reaches the page through `Entries`. An arm that does nothing is `()`.
- `bind:value={draft}`: the textarea and the signal are kept equal.
- `on:press={() => match remove(entry.id) { .. }}` inside a list row: the
  handler reads `entry` from its row. The compiler writes what a handler
  uses into the page, so the browser runs it without rendering the list
  again (ADR-0134).
- `<form method="get" action="/sign-in">`: plain HTML forms to the
  server's sign-in, sign-up and sign-out routes.

## 8. Check it

```sh
target/debug/pw check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/guestbook/*.pw
```

It ends with `no diagnostics`.

Make a mistake to see what the compiler says. Take the `{#if SignedIn}`
from around the Remove button, and `pw check` refuses the page, with
PW5048, naming the button and the predicate the page does not ask.

A clean check is not yet a build: `pw build` refuses some programs
`pw check` passes ([where `pw check` stops](tour-pages.md#where-pw-check-stops)).
The next step builds the app.

## 9. Run it

Build the app for the browser and start the development server with
accounts:

```sh
bash spikes/own-renderer/guestbook.sh
cd spikes/own-renderer
PORT=7741 PW_IDENTITY=dev-accounts ../../target/debug/pw-dev-server dist-guestbook
```

Open <http://127.0.0.1:7741/>. You see the feed's first post, "Hello,
feed.", by Ada, and a Sign in button. Press **Sign up**: the development
provider asks for a handle, a name and a password (it says it is not for
production). Back on the guestbook, write a message and press **Sign**.
The entry is shown at once, then as the server wrote it, with a Remove
button only you have. Open the guestbook in a second browser: your entry
is there, and when you remove it, it goes from both pages.

`spikes/own-renderer/e2e/guestbook.spec.mjs` does exactly this in
Chromium, Firefox and WebKit.

## What you used

Modules and imports, opaque types with invariants, records and sum types,
`Option` and `Result`, pure functions, effect rows, host bindings, a
source, events, queries, predicates, commands with optimistic updates, a
page with signals, markup blocks, handlers and a bound textarea. The
[tour](tour.md) takes each in turn.
