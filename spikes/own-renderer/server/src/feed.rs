//! **The feed's data** (ADR-0218): the posts and users of the feed reference
//! app, a Twitter-shaped program (`examples/feed`), kept in memory.
//!
//! What its contracts import, `feed:data/posts#…` and `feed:data/users#…`,
//! is this. A query reads it; a command stages its writes here, and they are
//! the feed's once the command's transaction commits.

use super::*;
use crate::identity::{Principal, Principals};

/// **What a node grants the feed's data**, wherever it is kept.
pub(crate) const GRANTS: &[&str] = &[
    "database.read<Post>",
    "database.write<Post>",
    "database.read<User>",
    "database.read<Follow>",
    "database.write<Follow>",
    // Track `uploads`: a session's attached image, read and discarded, and
    // the claim a command holds on it from its acquisition to its release.
    "database.read<Attached>",
    "database.write<Attached>",
    "resource.acquire<Upload>",
    "resource.release<Upload>",
    // Track `notifications` (ADR-0274): what others do that involves a
    // user, written with what they do.
    "database.read<Notification>",
    "database.write<Notification>",
    // Track `messages` (ADR-0279): a message, and each reader's mark.
    "database.read<Message>",
    "database.write<Message>",
];

/// One post as kept: its author by id, and what it replies to.
#[derive(Debug, Clone)]
struct Row {
    id: String,
    author: String,
    text: String,
    likes: i64,
    reply_to: Option<String>,
    /// Track `uploads`: the image it carries, kept with it.
    image: Option<crate::uploads::ImageRow>,
}

#[derive(Debug, Clone, Default)]
struct State {
    /// Oldest first.
    posts: Vec<Row>,
    /// A user's handle and name, by id.
    users: BTreeMap<String, (String, String)>,
    /// Who follows whom (ADR-0257), the follower first.
    follows: std::collections::BTreeSet<(String, String)>,
    /// Posts ever written, so that a post's id is never one a deleted post
    /// had.
    written: usize,
    /// Commits, which the materializer's row for the feed counts.
    commits: usize,
    /// Track `uploads`: the leases a post's image is committed from, where
    /// the program declares an upload.
    leases: Option<crate::uploads::Leases>,
    /// Track `notifications`: what others did that involves each user,
    /// oldest first, and how many were ever written, each its own id.
    notes: Vec<crate::notifications::Note>,
    noted: usize,
    /// Track `messages`: every message, and each reader's mark.
    messages: crate::messages::Messages,
}

/// What a command changes, staged until it commits.
#[derive(Debug, Clone)]
enum Change {
    Post(Row),
    Like(String),
    /// The follower follows the followee (ADR-0257).
    Follow(String, String),
    /// The follower follows the followee no more.
    Unfollow(String, String),
    /// A post and every reply under it (track `identity`, ADR-0258).
    Delete(String),
    /// A signed-in author's handle and name, as their provider gave them:
    /// the user the feed shows for them.
    User(String, (String, String)),
    /// Track `uploads`: an attached image discarded, which ends its lease
    /// once the command commits.
    Discard,
    /// Track `notifications`: a notification, written with the like, the
    /// reply or the follow it tells of.
    Notify(crate::notifications::Note),
    /// Track `notifications`: every notification of a user, read.
    ReadAll(String),
    /// Track `messages`: a message, and its sender's side read to it.
    Message(crate::messages::Sent),
    /// Track `messages`: a reader's conversation with another, read.
    ReadConversation(String, String),
}

pub(crate) struct FeedData {
    state: Mutex<State>,
    /// Each session's principal (track `identity`): whom a session posts
    /// as. Until the server hands its own, every session is its own guest.
    principals: std::sync::RwLock<Principals>,
}

impl FeedData {
    pub(crate) fn new() -> FeedData {
        let mut state = State::default();
        for (id, handle, name) in [("u-ada", "@ada", "Ada"), ("u-grace", "@grace", "Grace")] {
            state
                .users
                .insert(id.to_string(), (handle.to_string(), name.to_string()));
        }
        state.posts = vec![
            Row {
                id: "p1".into(),
                author: "u-ada".into(),
                text: "Hello, feed.".into(),
                likes: 2,
                reply_to: None,
                image: None,
            },
            Row {
                id: "p2".into(),
                author: "u-grace".into(),
                text: "Hello, Ada.".into(),
                likes: 0,
                reply_to: Some("p1".into()),
                image: None,
            },
            Row {
                id: "p3".into(),
                author: "u-ada".into(),
                text: "And a reply to the reply.".into(),
                likes: 1,
                reply_to: Some("p2".into()),
                image: None,
            },
        ];
        state.written = state.posts.len();
        FeedData {
            state: Mutex::new(state),
            principals: std::sync::RwLock::new(Principals::default()),
        }
    }
}

/// The user a session posts as where it has no principal: one of its own,
/// named for it.
pub(crate) fn user_of(session: &str) -> String {
    format!("u-{session}")
}

/// **Who `session` reads as** (track `identity`, ADR-0258), the program's
/// `Viewer`: signed in or not, to an account or as the guest model's guest,
/// and its name.
pub(crate) fn viewer_val(principal: Option<Principal>) -> Val {
    let (signed_in, account, name) = match principal {
        Some(p) => (true, !p.is_guest(), p.name),
        None => (false, false, String::new()),
    };
    Val::Record(vec![
        ("signed-in".into(), Val::Bool(signed_in)),
        ("account".into(), Val::Bool(account)),
        ("name".into(), Val::String(name)),
    ])
}

/// **A signed-in author, as the feed shows them**: the handle and name their
/// provider vouched for, written with their post. A guest has none, and is
/// named for their session.
pub(crate) fn profile_of(principal: &Principal) -> Option<(String, String)> {
    (!principal.is_guest()).then(|| (principal.handle.clone(), principal.name.clone()))
}

fn user_val(state: &State, id: &str) -> Val {
    user_record(id, state.users.get(id).cloned())
}

/// **A user as the program's `User` is**: its handle and name where it has
/// them. A session's own user, which signed up as no one, is a guest named
/// for its session, the same to every reader. Until ADR-0220 it was "You" to
/// every reader.
pub(crate) fn user_record(id: &str, known: Option<(String, String)>) -> Val {
    let (handle, name) = known.unwrap_or_else(|| {
        // A session's id is long since it is random (track `identity`): a
        // guest is named for its first ten characters.
        let session: String = id
            .strip_prefix("u-")
            .unwrap_or(id)
            .chars()
            .take(10)
            .collect();
        (format!("@{session}"), format!("Guest {session}"))
    });
    Val::Record(vec![
        ("id".into(), Val::String(id.to_string())),
        ("handle".into(), Val::String(handle)),
        ("name".into(), Val::String(name)),
    ])
}

/// A post as the program's `Post` is: its replies inside it, as deep as they
/// go, where `replies` asks for them.
fn post_val(state: &State, row: &Row, replies: bool) -> Val {
    let inner = if replies {
        state
            .posts
            .iter()
            .filter(|r| r.reply_to.as_deref() == Some(row.id.as_str()))
            .map(|r| post_val(state, r, true))
            .collect()
    } else {
        Vec::new()
    };
    Val::Record(vec![
        ("id".into(), Val::String(row.id.clone())),
        ("author".into(), user_val(state, &row.author)),
        ("text".into(), Val::String(row.text.clone())),
        ("likes".into(), Val::S64(row.likes)),
        ("replies".into(), Val::List(inner)),
        (
            "images".into(),
            crate::uploads::images_val(row.image.as_ref(), state.leases.as_ref()),
        ),
    ])
}

/// **A post as a timeline shows it** (ADR-0222), the program's `Item`: who
/// wrote it, what it says, its likes, and whether the reader wrote it
/// (track `identity`), which the timeline is private to.
fn item_val(state: &State, row: &Row, reader: &str) -> Val {
    Val::Record(vec![
        ("id".into(), Val::String(row.id.clone())),
        ("author".into(), user_val(state, &row.author)),
        ("text".into(), Val::String(row.text.clone())),
        ("likes".into(), Val::S64(row.likes)),
        ("mine".into(), Val::Bool(row.author == reader)),
        (
            "images".into(),
            crate::uploads::images_val(row.image.as_ref(), state.leases.as_ref()),
        ),
    ])
}

/// **Whether the feed knows `id`** (ADR-0257): a user with a row, or one
/// who wrote a post, follows or is followed. A session's guest is known once
/// it has done one.
fn known(state: &State, id: &str) -> bool {
    state.users.contains_key(id)
        || state.posts.iter().any(|r| r.author == id)
        || state.follows.iter().any(|(a, b)| a == id || b == id)
}

/// **What `reader` is to `user`** (ADR-0257), the program's `Relation`.
fn relation_val(state: &State, reader: &str, user: &str) -> Val {
    let case = if reader == user {
        "yourself"
    } else if state
        .follows
        .contains(&(reader.to_string(), user.to_string()))
    {
        "following"
    } else {
        "not-following"
    };
    Val::Variant(case.into(), None)
}

/// How many of a user's posts their page lists, newest first (ADR-0257).
pub(crate) const PROFILE_POSTS: usize = 20;

/// **A user's page** (ADR-0257), the program's `Profile`: who follow them,
/// whom they follow, and their newest posts that reply to none.
fn profile_val(state: &State, id: &str) -> Val {
    let followers = state.follows.iter().filter(|(_, b)| b == id).count();
    let following = state.follows.iter().filter(|(a, _)| a == id).count();
    let posts: Vec<Val> = state
        .posts
        .iter()
        .rev()
        .filter(|r| r.author == id && r.reply_to.is_none())
        .take(PROFILE_POSTS)
        .map(|r| {
            Val::Record(vec![
                ("id".into(), Val::String(r.id.clone())),
                ("text".into(), Val::String(r.text.clone())),
                ("likes".into(), Val::S64(r.likes)),
            ])
        })
        .collect();
    Val::Record(vec![
        ("user".into(), user_val(state, id)),
        ("followers".into(), Val::S64(followers as i64)),
        ("following".into(), Val::S64(following as i64)),
        ("posts".into(), Val::List(posts)),
    ])
}

pub(crate) fn not_found() -> Val {
    Val::Result(Err(Some(Box::new(Val::Variant("not-found".into(), None)))))
}

pub(crate) fn ok(v: Val) -> Val {
    Val::Result(Ok(Some(Box::new(v))))
}

/// The reads of `state`: the timeline, a thread, a session's user and who
/// it reads as.
fn reads_of(state: Arc<State>, principals: Principals) -> crate::data::Ops {
    let mut ops: crate::data::Ops = BTreeMap::new();
    let (s, p) = (state.clone(), principals.clone());
    ops.insert(
        "feed:data/posts#timeline".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(session), Val::S64(limit)] => {
                let reader = p.user_of(session);
                // Each an `Item`, a post as a timeline shows it (ADR-0222):
                // its replies are its thread's.
                let shown: Vec<Val> = s
                    .posts
                    .iter()
                    .rev()
                    .filter(|r| r.reply_to.is_none())
                    .take((*limit).max(0) as usize)
                    .map(|r| item_val(&s, r, &reader))
                    .collect();
                Ok(vec![Val::List(shown)])
            }
            other => Err(format!("posts#timeline received {other:?}")),
        }),
    );
    let s = state.clone();
    ops.insert(
        "feed:data/posts#thread".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(id)] => Ok(vec![match s.posts.iter().find(|r| r.id == *id) {
                Some(row) => ok(post_val(&s, row, true)),
                None => not_found(),
            }]),
            other => Err(format!("posts#thread received {other:?}")),
        }),
    );
    // **The timeline of those a session's user follows** (ADR-0257): each an
    // `Item`, its own posts and theirs, that reply to none.
    let (s, p) = (state.clone(), principals.clone());
    ops.insert(
        "feed:data/posts#following".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(session), Val::S64(limit)] => {
                let reader = p.user_of(session);
                let shown: Vec<Val> = s
                    .posts
                    .iter()
                    .rev()
                    .filter(|r| r.reply_to.is_none())
                    .filter(|r| {
                        r.author == reader
                            || s.follows.contains(&(reader.clone(), r.author.clone()))
                    })
                    .take((*limit).max(0) as usize)
                    .map(|r| item_val(&s, r, &reader))
                    .collect();
                Ok(vec![Val::List(shown)])
            }
            other => Err(format!("posts#following received {other:?}")),
        }),
    );
    let s = state.clone();
    ops.insert(
        "feed:data/users#profile".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(id)] => Ok(vec![match known(&s, id) {
                true => ok(profile_val(&s, id)),
                false => not_found(),
            }]),
            other => Err(format!("users#profile received {other:?}")),
        }),
    );
    let (s, p) = (state.clone(), principals.clone());
    ops.insert(
        "feed:data/users#relation".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(session), Val::String(user)] => {
                Ok(vec![relation_val(&s, &p.user_of(session), user)])
            }
            other => Err(format!("users#relation received {other:?}")),
        }),
    );
    let p = principals.clone();
    // Track `uploads`: the image a session attached, waiting for its post.
    ops.insert(
        "feed:data/uploads#attached".to_string(),
        crate::uploads::attached_op(state.leases.clone()),
    );
    ops.insert(
        "feed:data/users#of-session".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(session)] => Ok(vec![Val::String(p.user_of(session))]),
            other => Err(format!("users#of-session received {other:?}")),
        }),
    );
    // Track `notifications` (ADR-0274): a reader's notifications and how
    // many are unread, each by the reader's handle, which is their id on the
    // wire, and who wrote a post, an id that grants nothing.
    let s = state.clone();
    ops.insert(
        "feed:data/notifications#list".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(reader), Val::S64(limit)] => Ok(vec![Val::List(
                crate::notifications::listed(&s.notes, reader, *limit)
                    .into_iter()
                    .map(|n| crate::notifications::note_val(n, user_val(&s, &n.actor)))
                    .collect(),
            )]),
            other => Err(format!("notifications#list received {other:?}")),
        }),
    );
    let s = state.clone();
    ops.insert(
        "feed:data/notifications#unread".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(reader)] => Ok(vec![Val::S64(crate::notifications::unread(
                &s.notes, reader,
            ))]),
            other => Err(format!("notifications#unread received {other:?}")),
        }),
    );
    let s = state.clone();
    ops.insert(
        "feed:data/posts#author".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(id)] => Ok(vec![Val::String(
                s.posts
                    .iter()
                    .find(|r| r.id == *id)
                    .map(|r| r.author.clone())
                    .unwrap_or_default(),
            )]),
            other => Err(format!("posts#author received {other:?}")),
        }),
    );
    // Track `messages` (ADR-0279): a reader's conversation, their list and
    // count, each by the reader's handle, and who may message whom.
    let (u, k) = (state.clone(), state.clone());
    crate::messages::reads(
        &mut ops,
        Arc::new(state.messages.clone()),
        Arc::new(state.follows.clone()),
        Arc::new(move |id: &str| user_val(&u, id)),
        Arc::new(move |id: &str| known(&k, id)),
    );
    ops.insert(
        "feed:data/users#viewer".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(session)] => Ok(vec![viewer_val(principals.of(session))]),
            other => Err(format!("users#viewer received {other:?}")),
        }),
    );
    ops
}

/// **A post and every reply under it**, by id.
fn under(posts: &[Row], id: &str) -> std::collections::BTreeSet<String> {
    let mut gone = std::collections::BTreeSet::from([id.to_string()]);
    loop {
        let more: Vec<String> = posts
            .iter()
            .filter(|r| r.reply_to.as_ref().is_some_and(|to| gone.contains(to)))
            .filter(|r| !gone.contains(&r.id))
            .map(|r| r.id.clone())
            .collect();
        if more.is_empty() {
            return gone;
        }
        gone.extend(more);
    }
}

/// **A command's writes to the feed, staged** (ADR-0218): under the feed's
/// lock from the call to the commit, as the store's cart is.
pub(crate) struct Staging<'a> {
    state: std::sync::MutexGuard<'a, State>,
    staged: Arc<Mutex<Vec<Change>>>,
    ops: crate::data::Ops,
    /// Track `uploads`: the leases the command claimed, given back where it
    /// does not commit.
    claims: crate::uploads::Claiming,
}

impl crate::data::Staged for Staging<'_> {
    fn ops(&self) -> crate::data::Ops {
        self.ops.clone()
    }

    fn rows(&self) -> Option<Vec<(String, String)>> {
        let staged = self.staged.lock().expect("staged");
        (!staged.is_empty()).then(|| {
            vec![(
                "feed:posts".to_string(),
                (self.state.commits + 1).to_string(),
            )]
        })
    }

    /// Blob preparation occurs after the materializer's completion fence.
    fn prepare_materialized(&mut self) -> Result<(), String> {
        self.claims.lock().expect("claims").keep()
    }

    fn publish(&mut self) {
        let staged: Vec<Change> = self.staged.lock().expect("staged").drain(..).collect();
        if !staged.is_empty() {
            self.state.commits += 1;
        }
        // ADR-0261: the images of the posts it deletes.
        let mut released = std::collections::BTreeSet::new();
        for change in staged {
            match change {
                Change::Post(row) => {
                    self.state.written += 1;
                    self.state.posts.push(row);
                }
                Change::Delete(id) => {
                    let gone = under(&self.state.posts, &id);
                    released.extend(
                        self.state
                            .posts
                            .iter()
                            .filter(|r| gone.contains(&r.id))
                            .filter_map(|r| r.image.as_ref().map(|i| i.key.clone())),
                    );
                    self.state.posts.retain(|r| !gone.contains(&r.id));
                    // Track `notifications`: a deleted post's notifications
                    // go with it, and its replies'.
                    self.state
                        .notes
                        .retain(|n| !n.post.as_ref().is_some_and(|p| gone.contains(p)));
                }
                Change::Notify(mut note) => {
                    self.state.noted += 1;
                    note.id = format!("n{}", self.state.noted);
                    self.state.notes.push(note);
                }
                Change::ReadAll(reader) => {
                    for n in self
                        .state
                        .notes
                        .iter_mut()
                        .filter(|n| n.recipient == reader)
                    {
                        n.read = true;
                    }
                }
                Change::User(id, profile) => {
                    self.state.users.insert(id, profile);
                }
                Change::Message(sent) => self.state.messages.commit(sent),
                Change::ReadConversation(reader, with) => {
                    self.state.messages.read(&reader, &with);
                }
                Change::Like(id) => {
                    if let Some(row) = self.state.posts.iter_mut().find(|r| r.id == id) {
                        row.likes += 1;
                    }
                }
                Change::Follow(follower, followee) => {
                    self.state.follows.insert((follower, followee));
                }
                Change::Unfollow(follower, followee) => {
                    self.state.follows.remove(&(follower, followee));
                }
                Change::Discard => {}
            }
        }
        // Track `uploads`: each lease the command committed or discarded
        // ended, now that its transaction has.
        self.claims.lock().expect("claims").settle();
        // **A deleted post's image, collected where no post names it**
        // (ADR-0261), under the feed's lock, so no other command commits
        // between the look and the delete.
        if let Some(leases) = &self.state.leases {
            for key in released {
                let named = names(&self.state.posts, &key);
                if let Err(why) = leases.collect(&key, named) {
                    eprintln!("pw dev server: blob {key} was not collected: {why}");
                }
            }
        }
    }
}

/// **Whether a post names the blob `key`** (ADR-0261).
fn names(posts: &[Row], key: &str) -> bool {
    posts
        .iter()
        .any(|r| r.image.as_ref().is_some_and(|i| i.key == key))
}

impl FeedData {
    fn principals(&self) -> Principals {
        self.principals.read().expect("principals").clone()
    }
}

/// **The author `session` posts as, staged with their post**: the
/// principal's user, and, for a signed-in one, the handle and name the feed
/// shows for them.
fn author(principals: &Principals, session: &str, staged: &mut Vec<Change>) -> String {
    match principals.of(session) {
        Some(p) => {
            if let Some(profile) = profile_of(&p) {
                staged.push(Change::User(p.user.clone(), profile));
            }
            p.user
        }
        None => user_of(session),
    }
}

/// A post's id: `p` and how many were ever written, so never a deleted one's.
fn next_id(state: &State, staged: &[Change]) -> String {
    let posted = staged
        .iter()
        .filter(|c| matches!(c, Change::Post(_)))
        .count();
    format!("p{}", state.written + posted + 1)
}

impl crate::data::DataLayer for FeedData {
    fn names_blob(&self, key: &str) -> Result<bool, String> {
        Ok(names(&self.state.lock().expect("feed").posts, key))
    }

    fn reads(&self, _session: &str, _stopped: Option<Stopped>) -> crate::data::Ops {
        reads_of(
            Arc::new(self.state.lock().expect("feed").clone()),
            self.principals(),
        )
    }

    // TRACK SEAM (render-merge): a render's snapshot, the feed as its last
    // commit left it when the render began, and each of its queries' reads
    // over it (NEXT 4c).
    fn snapshot(&self) -> Option<crate::data::Snapshot> {
        Some(Box::new(Arc::new(self.state.lock().expect("feed").clone())))
    }

    fn reads_in(
        &self,
        snapshot: &crate::data::Snapshot,
        session: &str,
        stopped: Option<Stopped>,
    ) -> crate::data::Ops {
        match snapshot.downcast_ref::<Arc<State>>() {
            Some(state) => reads_of(state.clone(), self.principals()),
            None => self.reads(session, stopped),
        }
    }

    fn identified_by(&self, principals: Principals) {
        *self.principals.write().expect("principals") = principals;
    }

    fn begin<'a>(&'a self, _session: &str) -> Box<dyn crate::data::Staged + 'a> {
        let state = self.state.lock().expect("feed");
        let seen = Arc::new(state.clone());
        let staged: Arc<Mutex<Vec<Change>>> = Arc::default();
        let principals = self.principals();
        let mut ops = reads_of(seen.clone(), principals.clone());
        let (s, into, p) = (seen.clone(), staged.clone(), principals.clone());
        ops.insert(
            "feed:data/posts#publish".to_string(),
            Arc::new(move |args: &[Val]| match args {
                [Val::String(session), Val::String(text)] => {
                    let mut staged = into.lock().expect("staged");
                    let row = Row {
                        id: next_id(&s, &staged),
                        author: author(&p, session, &mut staged),
                        text: text.clone(),
                        likes: 0,
                        reply_to: None,
                        image: None,
                    };
                    let answer = ok(post_val(&s, &row, false));
                    staged.push(Change::Post(row));
                    Ok(vec![answer])
                }
                other => Err(format!("posts#publish received {other:?}")),
            }),
        );
        // **A reply** (ADR-0231): a post that replies to `to`, shown in its
        // thread and in no timeline. To a post that is not there, none.
        let (s, into, p) = (seen.clone(), staged.clone(), principals.clone());
        ops.insert(
            "feed:data/posts#reply".to_string(),
            Arc::new(move |args: &[Val]| match args {
                [Val::String(session), Val::String(to), Val::String(text)] => {
                    if !s.posts.iter().any(|r| r.id == *to) {
                        return Ok(vec![not_found()]);
                    }
                    let mut staged = into.lock().expect("staged");
                    let row = Row {
                        id: next_id(&s, &staged),
                        author: author(&p, session, &mut staged),
                        text: text.clone(),
                        likes: 0,
                        reply_to: Some(to.clone()),
                        image: None,
                    };
                    let answer = ok(post_val(&s, &row, false));
                    // Track `notifications`: the replied-to post's author is
                    // told, in the reply's transaction, unless it is theirs.
                    let to_author = s.posts.iter().find(|r| r.id == *to).map(|r| &r.author);
                    if let Some(note) = to_author.and_then(|a| {
                        crate::notifications::noted(
                            a,
                            &row.author,
                            crate::notifications::Act::Replied,
                            Some(&row.id),
                        )
                    }) {
                        staged.push(Change::Notify(note));
                    }
                    staged.push(Change::Post(row));
                    Ok(vec![answer])
                }
                other => Err(format!("posts#reply received {other:?}")),
            }),
        );
        // Track `uploads`: a command claims its session's attached image,
        // commits it with a post, or discards it.
        let claims = crate::uploads::Claims::new(seen.leases.clone());
        ops.insert(
            "feed:data/uploads#claim".to_string(),
            crate::uploads::claim_op(claims.clone()),
        );
        let into = staged.clone();
        ops.insert(
            "feed:data/uploads#discard".to_string(),
            crate::uploads::discard_op(claims.clone(), move || {
                into.lock().expect("staged").push(Change::Discard)
            }),
        );
        // **A post with its image** (track `uploads`): the image the
        // command's claim holds, kept with the post, by the author `publish`
        // writes as. A handle this command did not claim is not found.
        let (s, into, p, holding) = (
            seen.clone(),
            staged.clone(),
            principals.clone(),
            claims.clone(),
        );
        ops.insert(
            "feed:data/posts#publish-image".to_string(),
            Arc::new(move |args: &[Val]| match args {
                [
                    Val::String(session),
                    Val::String(text),
                    Val::String(alt),
                    Val::String(upload),
                ] => {
                    let Some(image) = holding.lock().expect("claims").commit(session, upload, alt)
                    else {
                        return Ok(vec![not_found()]);
                    };
                    let mut staged = into.lock().expect("staged");
                    let row = Row {
                        id: next_id(&s, &staged),
                        author: author(&p, session, &mut staged),
                        text: text.clone(),
                        likes: 0,
                        reply_to: None,
                        image: Some(image),
                    };
                    let answer = ok(post_val(&s, &row, false));
                    staged.push(Change::Post(row));
                    Ok(vec![answer])
                }
                other => Err(format!("posts#publish-image received {other:?}")),
            }),
        );
        // **A follow** (ADR-0257): once, however often; a follow again
        // writes what is there, and its event refreshes every page that
        // shows it. One the feed does not know, or the follower itself, is
        // not found.
        let (s, into, p) = (seen.clone(), staged.clone(), principals.clone());
        ops.insert(
            "feed:data/users#follow".to_string(),
            Arc::new(move |args: &[Val]| match args {
                [Val::String(session), Val::String(user)] => {
                    let follower = p.user_of(session);
                    if follower == *user || !known(&s, user) {
                        return Ok(vec![not_found()]);
                    }
                    let mut staged = into.lock().expect("staged");
                    // Track `notifications`: the one followed is told, once:
                    // following again is no change, and tells no one.
                    let new = !s.follows.contains(&(follower.clone(), user.clone()));
                    if new
                        && let Some(note) = crate::notifications::noted(
                            user,
                            &follower,
                            crate::notifications::Act::Followed,
                            None,
                        )
                    {
                        staged.push(Change::Notify(note));
                    }
                    staged.push(Change::Follow(follower, user.clone()));
                    Ok(vec![ok(Val::String(user.clone()))])
                }
                other => Err(format!("users#follow received {other:?}")),
            }),
        );
        // **An unfollow** (ADR-0257): one not followed is no change.
        let (s, into, p) = (seen.clone(), staged.clone(), principals.clone());
        ops.insert(
            "feed:data/users#unfollow".to_string(),
            Arc::new(move |args: &[Val]| match args {
                [Val::String(session), Val::String(user)] => {
                    if !known(&s, user) {
                        return Ok(vec![not_found()]);
                    }
                    into.lock()
                        .expect("staged")
                        .push(Change::Unfollow(p.user_of(session), user.clone()));
                    Ok(vec![ok(Val::String(user.clone()))])
                }
                other => Err(format!("users#unfollow received {other:?}")),
            }),
        );
        // **A post deleted** (track `identity`, ADR-0258): it and every reply
        // under it. Who may is `requires OwnsPost(post)`'s, evaluated before
        // the command runs; a post that is not there is not found.
        let (s, into) = (seen.clone(), staged.clone());
        ops.insert(
            "feed:data/posts#delete".to_string(),
            Arc::new(move |args: &[Val]| match args {
                [Val::String(_), Val::String(id)] => {
                    if !s.posts.iter().any(|r| r.id == *id) {
                        return Ok(vec![not_found()]);
                    }
                    into.lock()
                        .expect("staged")
                        .push(Change::Delete(id.clone()));
                    Ok(vec![ok(Val::String(id.clone()))])
                }
                other => Err(format!("posts#delete received {other:?}")),
            }),
        );
        // Track `notifications`: a user's notifications, read: how many
        // were unread.
        let (s, into) = (seen.clone(), staged.clone());
        ops.insert(
            "feed:data/notifications#read".to_string(),
            Arc::new(move |args: &[Val]| match args {
                [Val::String(reader)] => {
                    into.lock()
                        .expect("staged")
                        .push(Change::ReadAll(reader.clone()));
                    Ok(vec![ok(Val::S64(crate::notifications::unread(
                        &s.notes, reader,
                    )))])
                }
                other => Err(format!("notifications#read received {other:?}")),
            }),
        );
        // Track `messages` (ADR-0279): a message from the session's user,
        // written by the author a post is, and the sender's side read to it.
        // Who may is `requires MayMessage(to)`'s, evaluated before the
        // command runs.
        let (s, into, p) = (seen.clone(), staged.clone(), principals.clone());
        ops.insert(
            "feed:data/messages#send".to_string(),
            Arc::new(move |args: &[Val]| match args {
                [Val::String(session), Val::String(to), Val::String(text)] => {
                    let mut staged = into.lock().expect("staged");
                    let from = author(&p, session, &mut staged);
                    let before = staged
                        .iter()
                        .filter(|c| matches!(c, Change::Message(_)))
                        .count() as u64;
                    let sent = crate::messages::Sent {
                        seq: s.messages.next_seq(before),
                        from: from.clone(),
                        to: to.clone(),
                        text: text.clone(),
                        at: crate::messages::now(),
                    };
                    let answer = ok(crate::messages::message_val(
                        &sent.id(),
                        user_val(&s, &from),
                        text,
                        false,
                    ));
                    staged.push(Change::Message(sent));
                    Ok(vec![answer])
                }
                other => Err(format!("messages#send received {other:?}")),
            }),
        );
        // Track `messages`: a reader's conversation, read: how many were
        // unread.
        let (s, into) = (seen.clone(), staged.clone());
        ops.insert(
            "feed:data/messages#read".to_string(),
            Arc::new(move |args: &[Val]| match args {
                [Val::String(reader), Val::String(with)] => {
                    into.lock()
                        .expect("staged")
                        .push(Change::ReadConversation(reader.clone(), with.clone()));
                    Ok(vec![ok(Val::S64(s.messages.unread_from(reader, with)))])
                }
                other => Err(format!("messages#read received {other:?}")),
            }),
        );
        let (s, into, p) = (seen, staged.clone(), principals);
        ops.insert(
            "feed:data/posts#like".to_string(),
            Arc::new(move |args: &[Val]| match args {
                [Val::String(session), Val::String(id)] => {
                    match s.posts.iter().find(|r| r.id == *id) {
                        Some(row) => {
                            let mut liked = row.clone();
                            liked.likes += 1;
                            let mut staged = into.lock().expect("staged");
                            // Track `notifications`: the post's author is told,
                            // in the like's transaction, unless it is theirs.
                            if let Some(note) = crate::notifications::noted(
                                &row.author,
                                &p.user_of(session),
                                crate::notifications::Act::Liked,
                                Some(id),
                            ) {
                                staged.push(Change::Notify(note));
                            }
                            staged.push(Change::Like(id.clone()));
                            Ok(vec![ok(post_val(&s, &liked, false))])
                        }
                        None => Ok(vec![not_found()]),
                    }
                }
                other => Err(format!("posts#like received {other:?}")),
            }),
        );
        Box::new(Staging {
            state,
            staged,
            ops,
            claims,
        })
    }

    fn grants(&self) -> Vec<&'static str> {
        GRANTS.to_vec()
    }

    fn default_page(&self) -> Option<&'static str> {
        None
    }

    fn uploaded_by(&self, leases: crate::uploads::Leases) {
        self.state.lock().expect("feed").leases = Some(leases);
    }
}
