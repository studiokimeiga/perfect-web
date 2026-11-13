# A tour of Pleris, part 2: data on the server

[Part 1](tour.md) covered the language. This part covers how a program
reads and writes data: host bindings, tables, sources, queries, events,
commands, privacy and time. [Part 3](tour-pages.md) puts them on pages.

The examples are modelled on the reference apps: `examples/feed` (a social
feed) and `examples/store` (a food-delivery store).

## Host bindings and capabilities

A program does not open a database connection. It declares the operations
it uses, each as a function with no body, bound to an operation the server
supplies:

```pw
module menu.data

type Item = Item { id: String, name: String, price: Int }

fn menu(store: Int) -> List<Item> !{ database.read<Item> }
    host "shop:data/menus#for-store"

fn rename(item: String, name: String) -> Result<Item, String> !{ database.write<Item> }
    host "shop:data/menus#rename"
```

- The string names the operation, `namespace:package/interface#operation`,
  each part lowercase words joined by `-`, as WIT writes them (PW0335,
  ADR-0262).
- The effect row is the operation's contract. `database.read<Item>` is a
  capability the server must grant before it runs the code that calls it,
  and the compiler places that code where it can run: at the origin, never
  in the browser (ADR-0026).
- The platform's own operations are bound the same way: `current_session()`
  is `host "pw:host/session#read"` in the platform package.

**These operations are written in Rust**, in the development server's
data layers for the reference apps: the feed's, the store's and kiokun's.
A program uses one data layer, chosen by its operations' prefix (`feed:`,
`kiokun:`, otherwise the store's). The server checks at start that every
operation a program imports is supplied, but not its signature: a record
of the wrong shape is refused when the call is answered (ADR-0218,
ADR-0291). A program of your own declares its data instead, as tables.

## Tables

A `table` declares data a program keeps, in the module of its name, and
the compiler writes its reads and writes as that module's functions:

```pw
module Notes

type Note = Note { id: String, text: String, added: Int }

table Notes
    row     Note
    key     id
    order   added
    bounded 100
    written by anyone

query Newest() -> List<Note>
    consistency strong
{
    Notes.all()
}

command keep(note: Note) -> Result<(), Note>
    requires    Anyone
    transaction serializable
    invalidates Newest()
{
    Notes.insert(note)
}
```

- `row` is the record each row is, `key` the fields that find one, `order`
  how rows of one prefix of the key come back, and `bounded 100` the most
  rows the table holds, which is what lets it be read whole (`all()`).
- `written by anyone`: any reader's request writes any row, on purpose. A
  table a reader's command writes says who may write its rows (PW6920):
  this, or `owned by <field>`, the field holding each row's owner's
  `capability.UserId`. An owned table's key leads with its owner, and each
  read and write takes the reader's handle there, which only the platform
  makes, so a reader reaches their own rows alone; `public table` reads it
  as every reader's, written by its owner (ADR-0395). The
  [tutorial](tutorial.md)'s entries are owned by their author.
- The functions are `Notes.get(id)`, `Notes.get_many(ids)`, `Notes.all()`,
  `Notes.insert(note)` (refused, answering the row that holds its key),
  `Notes.put(note)` and `Notes.delete(id)`; an `index` adds reads by other
  fields, and a key of several fields reads of its prefixes. Each has its
  effect, `database.read<Notes>` or `database.write<Notes>`, and no other
  way reaches the rows (PW0362).
- A command's writes are one transaction, committed with its events and
  invalidations or not at all (PW5106 holds it to naming what goes stale).
- The development server serves every program's tables with one generic
  adapter, in memory, through a gate that links each component to its own
  tables' functions alone (ADR-0338, ADR-0385, ADR-0386). The
  [tutorial](tutorial.md)'s guestbook is built this way.

## Sources

A `source` states what the database behind some types guarantees:

```pw
module feed.data

type Post = Post { id: String, text: String }

type User = User { id: String, name: String }

source FeedData
    holds        Post, User
    transactions serializable
    reads        strong
    changes      none

fn post(id: String) -> Post !{ database.read<Post> }
    host "feed:data/posts#get"
```

- `transactions`: `serializable`, `snapshot`, `read_committed` or `none`.
- `reads`: `strong`, `snapshot`, `read_your_writes` or `eventual`.
- `changes`: `none`, or `feed` where the database tells of its changes.

The compiler holds the program to it: a query cannot ask for more
consistency than the source's reads give (PW0344), a command cannot write
to two sources in one transaction (PW0345), and a command that announces
events needs transactions (PW0347). The server compares the source with
the database it opens, and does not serve on a mismatch (ADR-0207,
ADR-0246).

```pw fails=PW0344
module feed.loose

type Post = Post { id: String, text: String }

source Cache
    holds        Post
    transactions none
    reads        eventual
    changes      none

fn post(id: String) -> Post !{ database.read<Post> }
    host "feed:data/posts#get"

query Latest(id: String) -> Post
    consistency strong
    key         id
{
    post(id)
}
```

## Queries

A page reads data only through a `query`: a declared, keyed, cached read.

```pw
module store.menu

type Item = Item { id: String, name: String, price: Int }

event MenuChanged(store: Int)

fn menu(store: Int) -> List<Item> !{ database.read<Item> }
    host "shop:data/menus#for-store"

public query Menu(store: Int) -> List<Item>
    freshness      30.seconds
    consistency    snapshot
    cache          shared
    key            store
    invalidates_on MenuChanged(store)
    concurrency    one_per_key
    on_key_change  cancel
    timeout        2.seconds
{
    menu(store)
}
```

| clause | what it says |
|---|---|
| `freshness` | how old a cached value may be: `0.seconds`, `30.seconds`, `5.minutes` |
| `consistency` | what the read must see: `strong`, `snapshot`, `read_your_writes`, `eventual` |
| `cache` | `shared` between readers, or `private` to one |
| `key` | the parameters that make an entry: the body's every read of them |
| `invalidates_on` | the events that make an entry stale |
| `concurrency` | `one_per_key` request at a time, or `parallel` |
| `on_key_change` | when the page's key moves on: `cancel` the old read, `supersede`, or `keep` |
| `timeout` | how long a read may take |
| `retry`, `fallback` | bounded retries, and what to show when the origin fails |

No clause is required on every query, but some go together. A query a
page's signal keys says what happens to the read for its old key
(`on_key_change`, PW5309), because that key changes while the page is open;
a `session` query is never stale (`freshness 0.seconds`, PW0102). The
compiler checks `consistency`; the development server does not enforce it
at run time yet.

**Live updates.** When a command announces `MenuChanged(47)`, every cached
entry of `Menu(47)` is dropped, and every open page that shows it is read
again and sent what changed, without a reload (ADR-0219, ADR-0297). `_` in
an event's place matches any value: `invalidates_on MenuChanged(_)`.

## Commands

A `command` is the one way to change data. It runs at the origin, in one
transaction:

```pw
module feed.posts

import List
import String
import context.{ current_session }
import capability.{ Session, SessionId, UserId }

opaque type PostId = String

opaque type InteractionId = String

opaque type PostText = String where String.length(value) >= 1 & String.length(value) <= 280

type User = User { id: UserId, name: String }

type Item = Item { id: PostId, author: User, text: String }

type Post = Post { id: PostId, text: String }

type FeedError =
    | Unavailable
    | NotFound

event Posted(author: UserId)

predicate SignedIn
    says "Sign in to post."

fn timeline(s: Session<SessionId>, limit: Int) -> List<Item> !{ database.read<Post> }
    host "feed:data/posts#timeline"

fn publish(s: Session<SessionId>, text: PostText) -> Result<Post, FeedError> !{ database.write<Post> }
    host "feed:data/posts#publish"

fn author(s: Session<SessionId>) -> UserId !{ database.read<User> }
    host "feed:data/users#of-session"

session query Timeline(session: Session<SessionId>, limit: Int) -> List<Item>
    freshness      0.seconds
    consistency    read_your_writes
    cache          private
    key            session, limit
    invalidates_on Posted(_)
    concurrency    one_per_key
    on_key_change  cancel
    timeout        2.seconds
{
    timeline(session, limit)
}

command post(text: PostText) -> Result<Post, FeedError>
    requires      SignedIn
    idempotent_by InteractionId
    emits         Posted(author(current_session()))
    optimistic    Timeline(current_session(), _) as feed => pending(feed, text)
{
    publish(current_session(), text)
}

fn pending(feed: List<Item>, text: PostText) -> List<Item> !{} {
    let you = User { id: UserId("you"), name: "You" }
    List.concat([Item { id: PostId("pending"), author: you, text: text.value }], feed)
}
```

- `requires SignedIn`: a predicate the server checks before the body runs.
  A refusal is answered 403, and the page says the predicate's words where
  the button was (ADR-0302). Every command a reader can run says who may,
  since the server runs it for any request that names it, a page or none
  (PW0367): `requires` and its predicates, or `requires Anyone`, alone, for
  a command anyone may run. A command the platform runs says `invoked_by`
  instead.
- `idempotent_by InteractionId`: each press carries an interaction, and a
  second delivery of it is answered from the first, not run again
  (ADR-0121). Required for a command a page sends (PW0338).
- `emits Posted(..)`: an event, written in the command's own transaction,
  so it is sent if and only if the writes commit.
- `invalidates Timeline(current_session(), _)`, a clause this command does
  not use: drops entries directly, as an event would.
- `optimistic Timeline(..) as feed => pending(feed, text)`: the page shows
  `pending`'s value at once. `pending` must be pure (PW0330). If the
  command fails, the platform puts back what the page held: there is no
  rollback to write (PW0327).

The compiler also holds a command to its readers: a command that writes
`Post` must reach every query that reads `Post`, through an event the query
listens for or an `invalidates`, or a page would show a stale value
(PW5106). A query fresh for some seconds (`freshness 5.seconds`) is let
go stale on purpose, and is exempt. Here nothing tells `Latest`:

```pw fails=PW5106
module feed.stale

import context.{ current_session }
import capability.{ Session, SessionId }

opaque type InteractionId = String

type Post = Post { id: String, text: String }

type FeedError =
    | Unavailable

fn latest() -> List<Post> !{ database.read<Post> }
    host "feed:data/posts#latest"

fn publish(s: Session<SessionId>, text: String) -> Result<Post, FeedError> !{ database.write<Post> }
    host "feed:data/posts#publish"

public query Latest() -> List<Post>
    freshness 0.seconds
    cache     shared
{
    latest()
}

command post(text: String) -> Result<Post, FeedError>
    requires      Anyone
    idempotent_by InteractionId
{
    publish(current_session(), text)
}
```

And a command that shows a value before the server answers (`optimistic`)
must also invalidate what it speculated on (PW5107), even where the query is
let go stale. Here `Latest` is fresh for five seconds, which exempts it from
the rule above, and nothing tells it what the speculation was about:

```pw fails=PW5107
module feed.speculative

import context.{ current_session }
import capability.{ Session, SessionId }

opaque type InteractionId = String

type Post = Post { id: String, text: String }

type FeedError =
    | Unavailable

fn latest() -> List<Post> !{ database.read<Post> }
    host "feed:data/posts#latest"

fn publish(s: Session<SessionId>, text: String) -> Result<Post, FeedError> !{ database.write<Post> }
    host "feed:data/posts#publish"

public query Latest() -> List<Post>
    freshness 5.seconds
    cache     shared
{
    latest()
}

fn with_post(posts: List<Post>, text: String) -> List<Post> !{} {
    posts
}

command post(text: String) -> Result<Post, FeedError>
    requires      Anyone
    idempotent_by InteractionId
    optimistic    Latest() as posts => with_post(posts, text)
{
    publish(current_session(), text)
}
```

A command is called only by a page's handler (PW0339), and the handler
learns only whether it committed (PW0620): the new value reaches the page
through the query.

## Who may: predicates

```pw
module feed.rules

opaque type PostId = String

predicate SignedIn
    says "Sign in to post, reply or like."

predicate OwnsPost(post: PostId)
    says "Only its author can delete a post."
```

A predicate's meaning is the deployment's, not the program's: the program
names it, and says what a refused reader is told. The development server
evaluates `SignedIn`, `OwnsPost(post)` and `MayMessage(to)`, and refuses to
start a program that requires any other.

## Whose data: privacy labels

Every query and page says whose data it shows:

- no keyword: public, the same for every reader;
- `session`: one browser session's;
- `user`: one signed-in user's, in each of their sessions.

`current_session()` and `current_user()` are the platform's answers to "who
is reading", and nothing else can make them. A value read with one carries
its label through every call, branch and return, and a labelled value
cannot reach a shared cache (PW5001), a public page, or a log or a browser
it must not (ADR-0128, ADR-0129, ADR-0263, ADR-0270). Neither is a
credential: `current_session()` is a one-way digest of the session, never
the cookie the browser holds, so whatever a program keys by it, emits or
keeps opens no session.

```pw
module feed.unread

import capability
import capability.{ UserId }

type Notification = Notification { id: String, unread: Bool }

event Notified(user: UserId)

fn unread_count(reader: capability.User<UserId>) -> Int !{ database.read<Notification> }
    host "feed:data/notifications#unread"

user query Unread(reader: capability.User<UserId>) -> Int
    freshness      0.seconds
    consistency    read_your_writes
    cache          private
    key            reader
    invalidates_on Notified(reader)
    concurrency    one_per_key
    on_key_change  cancel
    timeout        2.seconds
{
    unread_count(reader)
}
```

`Notified(reader)` matches an event whose `UserId` is the reader's: one
user's notification drops only that user's entries, in each of their
sessions (ADR-0270). A page reads it with `query Unread(current_user())`.

## Time

There is no `Date.now()` in rendering code. A value that depends on the
clock asks the clock a question the platform can answer and remember:

```pw
module store.hours

import clock
import clock.{ Instant }

type Shift = Shift { opens: Instant, closes: Instant }

fn open_now(shift: Shift) -> Bool !{ clock.compare } {
    clock.passed(shift.opens) & !clock.passed(shift.closes)
}
```

`clock.passed(at)` is true once `at` has passed. A cached value or a page
that asked it is kept until that instant, then read again and sent to every
page that shows it (ADR-0304). `clock.now()` and `now_wall()` exist and
answer `0` (`packages/pw-platform-web/clock.pw`): no host implements a clock
read yet.

A command can schedule another for later, in its own transaction:

```pw
module orders.reminders

import clock.{ minutes }

opaque type InteractionId = String

command remind(order: String) -> Result<Int, String>
    invoked_by    Clock
    idempotent_by InteractionId
{
    Ok(1)
}

command remind_later(order: String) -> Result<Int, String> !{ clock.schedule }
    requires Anyone
{
    schedule remind(order) after minutes(30)
    Ok(1)
}
```

`invoked_by Clock`: no reader presses `remind`; the platform runs it once
the time has passed (ADR-0334). Durations are `minutes(30)` in code and
`30.minutes` in a clause.
