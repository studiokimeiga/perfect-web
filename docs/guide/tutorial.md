# Tutorial: a guestbook

<!-- guide-program: examples/guestbook -->

This tutorial builds a small app end to end: a guestbook. Anyone can read
it. A reader signed in to an account can sign it, and remove what they
wrote. Every open page shows a new entry, and a removed one goes, without
a reload. It keeps at most 1,000 entries, and says so when it is full.

You write four files:

- `guestbook/Books.pw` and `guestbook/Entries.pw`: the data, declared as
  tables;
- `guestbook/book.pw`: the queries and commands over them;
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

**Where the data lives.** A program declares its data as tables: what a
row holds, the fields that find it, and how it is read. The compiler
writes each table's reads and writes as functions of its module, and the
development server serves every program's tables with one generic adapter
(ADR-0338). So the guestbook has no data layer of its own to write, in
Pleris or in anything else. That adapter keeps the tables in memory: they
are gone when the server stops. A database behind the same declarations
is the next step (PostgreSQL first).

## 1. The words of an entry

A table is declared in the module of its name, one table a module. The
entries' module starts with what it uses:

```pw file=guestbook/Entries.pw
module Entries

import String
import capability.{ UserId }
import Books
```

`import String` gives the standard library's module, so you write
`String.trim(s)`. `import capability.{ UserId }` gives one name of the web
platform's, used bare: the type of a user's id. `import Books` is the other
table's module (step 2).

An entry's text is not any string. It is something, and at most 280
characters. Say so once, as a type:

```pw file=guestbook/Entries.pw
opaque type Words = String where String.length(value) >= 1 & String.length(value) <= 280

fn words(draft: String) -> Option<Words> !{} {
    let text = String.trim(draft)
    if String.length(text) >= 1 & String.length(text) <= 280 {
        Some(Words(text))
    } else {
        None
    }
}

fn text_of(words: Words) -> String !{} {
    words.value
}
```

- `opaque type Words = String` is a new type whose values are strings
  inside. Outside its module nothing can mistake a `Words` for any other
  string, and only its module reads the string inside (`words.value`),
  which `text_of` hands out.
- `where ...` is the type's invariant. The compiler checks every place
  that makes a `Words` (`Words(text)`), and here it is shown to hold by
  the `if` around it. Construct one anywhere the condition is not known,
  and the compiler refuses it (PW0622).
- `words` returns `Option<Words>`: `Some(words)` or `None`. There is no
  `null`.
- `!{}` is the function's effect row, empty: the function is pure, and
  the compiler holds it to that.

## 2. The tables

A guestbook is a book of entries. The book counts the entries ever signed
in it, so each new entry is numbered after every one before it:

```pw file=guestbook/Books.pw
module Books

type Book = Book { name: String, signed: Int }

table Books
    row Book
    key name
```

```pw file=guestbook/Entries.pw
type Entry = Entry { book: String, number: Int, author: UserId, name: String, text: Words }

table Entries
    row     Entry
    key     book, number
    refers  book to Books
    bounded 1000

fn most() -> Int !{} {
    1000
}
```

- `row` names the record each row is. Its fields are what the table
  stores: an entry's text is stored as `Words`, so no row can hold an empty
  text, or one longer than 280 characters. An entry's author is a `UserId`,
  an id that names someone and grants nothing: a row never holds a reader's
  handle, which every reader of the table would then read (PW6901).
- `key` is the fields that find one row: a book by its name, an entry by
  its book and its number. No two rows have one key.
- `refers book to Books`: an entry's book is a row of `Books`. A commit
  that would leave an entry of no book is refused.
- `bounded 1000`: the table holds at most 1,000 entries, and a write past
  that is refused. The page reads every entry (step 4), so the bound is
  what keeps that read, and the page, the same size however many sign.
  `most` says the same number to the code that signs (step 5).

From these the compiler writes each table's functions, which are the
program's only way to its rows:

| Function | What it does |
| --- | --- |
| `Books.get(name)` | the book of that name, or `None` |
| `Books.put(book)` | the book, whole, written by its key |
| `Entries.get(book, number)` | the entry of that key, or `None` |
| `Entries.of_book(book)` | every entry of a book, in the key's order |
| `Entries.all()` | every entry, of a table that states `bounded` |
| `Entries.insert(entry)` | a new entry: `Err(row)` where its key is held |
| `Entries.delete(book, number)` | the entry of that key, gone |

Each function has the effect it does: `Entries.of_book` performs
`database.read<Entries>`, and `Entries.insert` `database.write<Entries>`.
A function that calls one performs it too, and the server grants it only
where the code may run (at the server, never in the browser). And each set
form you do not use here, `get_many` and `of_book_many`, reads several keys
in one call.

The number of an entry is the book's count, so a removed entry's number is
never given again, as it would be by counting the entries there are.

## 3. What the page is told

The queries and commands are in the third file:

```pw file=guestbook/book.pw
module guestbook.book

import List
import context.{ current_user, account_of, Account }
import capability.{ User, UserId }
import Books.{ Book }
import Books
import Entries.{ Entry, Words, text_of, most }
import Entries
```

```pw file=guestbook/book.pw
opaque type InteractionId = String

type Line = Line { number: Int, name: String, text: String, mine: Bool }

type Viewer = Viewer { signed_in: Bool, name: String }
```

`Line` is an entry as the page shows it, with `mine` true where the reader
wrote it. `Viewer` is who reads. `InteractionId` is the type a command's
repeated sends are told apart by (step 5).

```pw file=guestbook/book.pw
type BookError =
    | SignedOut
    | Full
    | NotFound
    | NotYours
    | Unavailable
```

`BookError` is a *sum type*: a value is one of its cases. A `match` on it
must handle every case.

```pw file=guestbook/book.pw
event Signed(book: String)

event Removed(number: Int)
```

Events are facts the app's commands announce: someone signed the book, an
entry was removed. Queries name the events that make them stale (step 4).

```pw file=guestbook/book.pw
source Guestbook
    holds        Books, Entries
    transactions serializable
    reads        strong
    changes      none
```

A `source` states what the database holding these tables guarantees:
serializable transactions, reads of the latest commit, and no feed of
changes of its own. The compiler holds every query and command to it (a
query cannot ask for more than its source's reads give), and the server
compares it with the database it opens before serving anything (ADR-0207,
ADR-0246).

```pw file=guestbook/book.pw
fn guestbook() -> String !{} {
    "guestbook"
}

fn viewer(account: Option<Account>) -> Viewer !{} {
    match account {
        Some(account) => Viewer { signed_in: true, name: account.name },
        None => Viewer { signed_in: false, name: "" },
    }
}

fn mine(entry: Entry, reader: Option<Account>) -> Bool !{} {
    match reader {
        Some(account) => entry.author == account.id,
        None => false,
    }
}

fn line(entry: Entry, reader: Option<Account>) -> Line !{} {
    Line { number: entry.number, name: entry.name, text: text_of(entry.text), mine: mine(entry, reader) }
}

fn refused(error: BookError) -> String !{} {
    match error {
        SignedOut => "Sign in to sign the guestbook.",
        Full => "The guestbook is full: it keeps 1,000 entries.",
        NotFound => "That entry is gone.",
        NotYours => "Only its author removes an entry.",
        Unavailable => "The guestbook was not signed. Try again.",
    }
}
```

The app keeps one book, named `"guestbook"`. `viewer`, `mine` and `line`
are pure: what the page is told, made from what the tables and the
platform answer. An `Account` is the platform's: a signed-in reader's id
and the name their sign-in provider gave them (step 4). `refused` is what
the page says when a command answers one of its errors (step 6).

## 4. Queries the page reads

A page never reads a table itself. It reads a `query`: a declared, keyed
read, with a freshness, a cache and a timeout.

```pw file=guestbook/book.pw
user query Me(reader: User<UserId>) -> Viewer
    freshness     0.seconds
    consistency   read_your_writes
    cache         private
    key           reader
    concurrency   one_per_key
    on_key_change cancel
    timeout       2.seconds
{
    viewer(account_of(reader))
}
```

```pw file=guestbook/book.pw
user query Latest(reader: User<UserId>, limit: Int) -> List<Line>
    freshness      0.seconds
    consistency    read_your_writes
    cache          private
    key            reader, limit
    invalidates_on Signed(_), Removed(_)
    concurrency    one_per_key
    on_key_change  cancel
    timeout        2.seconds
{
    let account = account_of(reader)
    List.map(List.take(List.reverse(Entries.of_book(guestbook())), limit), e => line(e, account))
}
```

- `user query` says the result is the reader's: it is cached privately,
  per reader, and never in a cache another reader shares. `reader` is the
  reader's handle, `User<UserId>`, which only the platform makes (the page
  passes `current_user()`).
- `account_of(reader)` is the account the reader signed in to: the id their
  handle names, a plain `UserId` the program can store in a row and compare
  with one, and the name their sign-in provider gave them; `None` where no
  one signed in to an account, a guest's session and a signed-out reader's
  among them. Whose account it is says who reads, so the compiler treats
  it as the reader's too: return it from a query cached for every reader
  and it refuses the query (PW5001).
- `Entries.of_book` reads every entry of the book, in number order;
  `List.reverse` puts the newest first, and `List.take` the first `limit`.
  Pleris has no paged read of a table yet, so the guestbook is read whole
  each time, which is why `Entries` states its bound.
- `invalidates_on Signed(_), Removed(_)`: when a command announces either
  event, every open page that shows this query is read again and told the
  new value, live. This is how the guestbook updates without a reload.
- `on_key_change cancel`: when the page asks for another `limit`, the old
  read is cancelled rather than left to finish.

## 5. Commands that write

A `command` is the one way a page changes data. First, who may:

```pw file=guestbook/book.pw
predicate SignedIn
    says "Sign in to sign the guestbook."
```

A predicate's meaning is the server's: the development server knows
`SignedIn` (the session has signed in). The program says, in its own
words, what a reader is told when it refuses.

```pw file=guestbook/book.pw
command sign(text: Words) -> Result<Entry, BookError>
    requires      SignedIn
    idempotent_by InteractionId
    emits         Signed(guestbook())
    optimistic    Latest(current_user(), _) as shown => pending(shown, text)
{
    match account_of(current_user()) {
        None => Err(SignedOut),
        Some(account) => if List.length(Entries.all()) >= most() {
            Err(Full)
        } else {
            let number = match Books.get(guestbook()) {
                Some(book) => book.signed + 1,
                None => 1,
            }
            Books.put(Book { name: guestbook(), signed: number })
            let entry = Entry { book: guestbook(), number: number, author: account.id, name: account.name, text: text }
            match Entries.insert(entry) {
                Ok(_) => Ok(entry),
                Err(_) => Err(Unavailable),
            }
        },
    }
}

fn pending(shown: List<Line>, text: Words) -> List<Line> !{} {
    let entry = Line { number: 0 - 1 - List.length(shown), name: "You", text: text_of(text), mine: false }
    List.concat([entry], shown)
}
```

- The body is one transaction: it reads the book's count, writes it one
  higher, and inserts the entry. They commit together or not at all, and
  no other command sees either before they do. Two readers signing at once
  are given two numbers.
- A reader with no account (no one signed in, or a guest of a development
  server that names every session) is answered `SignedOut`: an entry names
  its author by their account.
- A full guestbook is answered `Full`, before anything is written: the
  table would refuse the 1,001st entry anyway, but as a failed write, which
  says nothing a reader can read. `Full` is the program's own answer, and
  the page says it (step 6).
- `Entries.insert` answers `Err(row)` where the key is held already, and
  the compiler makes the program handle it. Here it cannot be (the number
  was just counted), so it answers `Unavailable`, and a declared error
  commits nothing, the count's write with it.
- `requires SignedIn`: checked by the server before the body runs. A
  refused press is answered 403, and the page says the predicate's words
  where the button was.
- `idempotent_by InteractionId`: a press sent twice (a retry, a double
  click) commits once.
- `emits Signed(...)`: the event is written with the entry, in the same
  transaction, so it is sent if and only if the entry is kept.
- `optimistic Latest(...) as shown => pending(shown, text)`: the page
  shows the entry at once, before the server answers. `pending` is a pure
  function of the list the page holds. If the command fails, the platform
  puts back the list it held: there is no rollback to write.

The pending entry has a number the server never gives, and `mine: false`,
so it offers no Remove button until the server's row takes its place.

```pw file=guestbook/book.pw
command remove(number: Int) -> Result<Int, BookError>
    requires      SignedIn
    idempotent_by InteractionId
    emits         Removed(number)
{
    match Entries.get(guestbook(), number) {
        None => Err(NotFound),
        Some(entry) => if mine(entry, account_of(current_user())) {
            Entries.delete(guestbook(), number)
            Ok(number)
        } else {
            Err(NotYours)
        },
    }
}
```

Only the author can remove an entry: the command reads it and compares its
author with the reader's account, in the same transaction as the delete,
so the entry it deletes is the entry it found theirs. Another reader's
press answers `NotYours`, and nothing commits.

## 6. The page

The last file holds the page:

```pw file=guestbook/pages.pw
module guestbook.pages

import List
import String
import context.{ current_user }
import Entries.{ words }
import guestbook.book.{ Me, Latest, SignedIn, sign, remove, refused }
```

```pw file=guestbook/pages.pw
user page Book() {
    route     "/"
    placement origin
    cache     private

    signal shown: Int = 20
    signal draft: String = ""
    signal said: String = ""
    let me = query Me(current_user())
    let entries = query Latest(current_user(), shown)

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
            <div hidden={!me.signed_in}>
                <form hidden={!SignedIn} on:submit|prevent={() => match words(draft) {
                    Some(text) => match sign(text) {
                        Ok(_) => {
                            draft = ""
                            said = ""
                        },
                        Err(error) => said = refused(error),
                    },
                    None => (),
                }}>
                    <label>Your message <textarea bind:value={draft}></textarea></label>
                    <button type="submit" disabled={String.length(draft) == 0}>Sign</button>
                </form>
                <p id="said" role="status">{said}</p>
            </div>
            {#if List.length(entries) == 0}<p id="empty">No one has signed yet.</p>{/if}
            <ul>
                {#each entries as entry (entry.number)}
                    <li>
                        <p>{entry.text}</p>
                        <p class="by">{entry.name}</p>
                        {#if SignedIn}{#if entry.mine}<button type="button" on:press={() => match remove(entry.number) {
                            Ok(_) => said = "",
                            Err(error) => said = refused(error),
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

- `user page Book()` at `route "/"`: the page is the reader's, rendered at
  the origin for each reader, and cached privately. It is a `user` page
  because its rows are the reader's (`mine` was computed with their id),
  and a Remove button's handler holds one: a page that names no reader
  could not give it one (PW5007).
- `signal shown: Int = 20`, `signal draft: String = ""` and `signal said:
  String = ""` are the page's own state, in the browser. A handler assigns
  them; the page shows them.
- `let me = query Me(current_user())` reads a query. The page renders on
  the server with its value, and the browser is told when it changes.
- `{#if me.signed_in} .. {:else} .. {/if}`, `{#each entries as entry
  (entry.number)} .. {/each}`: markup blocks, as in Svelte. A list is
  keyed, so each row keeps its place as rows come and go.
- `<div hidden={!me.signed_in}>`: the composer is shown to a reader with an
  account, the one `sign` takes an entry from. It is hidden, not inside the
  `{#if}`: the browser renders again what reads a signal (`draft`, `said`)
  only outside a block a query's value decides (ADR-0137), and the compiler
  says so where it is not.
- `hidden={!SignedIn}` and `{#if SignedIn}`: the page asks the predicate
  of its reader, as the server will. A control that sends a command which
  `requires SignedIn` must be shown only where the page asks it: the
  compiler refuses it otherwise (PW5048), so a signed-out reader is never
  given a button that can only be refused.
- `on:submit|prevent={() => ...}`: the handler, run in the browser. It
  makes `Words` from the draft, calls the command, and matches on the
  answer: committed, it clears the draft, and the new entry reaches the
  page through `Latest`; refused, `said` takes the words `refused` gives
  the error, which the page shows in `<p id="said" role="status">`, so a
  full guestbook, or a reader signed out meanwhile, is told and not left
  wondering. An arm of two statements is a block, `{ .. }`; an arm that
  does nothing is `()`.
- `bind:value={draft}`: the textarea and the signal are kept equal.
- `on:press={() => match remove(entry.number) { .. }}` inside a list row:
  the handler reads `entry` from its row. The compiler writes what a
  handler uses into the page, so the browser runs it without rendering the
  list again (ADR-0134).
- `<form method="get" action="/sign-in">`: plain HTML forms to the
  server's sign-in, sign-up and sign-out routes.

## 7. Check it

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

## 8. Run it

Build the app for the browser and start the development server with
accounts:

```sh
bash spikes/own-renderer/guestbook.sh
cd spikes/own-renderer
PORT=7741 PW_IDENTITY=dev-accounts ../../target/debug/pw-dev-server dist-guestbook
```

The build writes `tables.json` beside the app: its tables, their fields,
keys and functions, which the server serves them from. Open
<http://127.0.0.1:7741/>. You see "No one has signed yet." and a Sign in
button. Press **Sign up**: the development provider asks for a handle, a
name and a password (it says it is not for production). Back on the
guestbook, write a message and press **Sign**. The entry is shown at once,
then as the server wrote it, under your name, with a Remove button only
you have. Open the guestbook in a second browser: your entry is there, and
when you remove it, it goes from both pages.

`spikes/own-renderer/e2e/guestbook.spec.mjs` does exactly this in
Chromium, Firefox and WebKit.

## What you used

Tables and the functions the compiler writes for them, modules and
imports, opaque types with invariants, records and sum types, `Option` and
`Result`, pure functions, effect rows, a source, events, queries keyed by
their reader, predicates, commands with optimistic updates, a page with
signals, markup blocks, handlers and a bound textarea. The
[tour](tour.md) takes each in turn.
