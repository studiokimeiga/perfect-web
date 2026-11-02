//! **The reader's user, and what others do that involves them** (track
//! `notifications`, ADR-0274; docs/PARALLEL.md, the integrator's rulings of
//! 2026-10-07).
//!
//! The typed principal: `context.current_user()` is the platform's
//! operation `pw:host/principal#read`, which the host answers beside
//! `pw:host/session#read`, from the identity's principals. Its value is a
//! handle, `User<UserId>`, and on the wire it is the user's id (a privacy
//! qualifier is ABI-transparent): the principal's `user`, or the session's
//! guest where no one signed in.

use super::*;
use crate::identity::Principals;

/// **The platform's principal operation**, by its key.
pub(crate) const PRINCIPAL_READ: &str = "pw:host/principal#read";

/// **The user `session` reads as**: its principal's, or its guest's. What
/// `current_user()` answers, and what a page's binding is given for it.
pub(crate) fn user_of(principals: &Principals, session: &str) -> String {
    principals.user_of(session)
}

/// **The platform's principal operation for one request**: the session's
/// user, and nothing else the component could ask it for, as the session
/// operation answers the session.
pub(crate) fn principal_operation(principals: &Principals, session: &str) -> HostFn {
    let user = user_of(principals, session);
    Arc::new(move |_args: &[Val]| Ok(vec![Val::String(user.clone())]))
}

/// **The platform's account operation**, by its key (track `data-path`):
/// `context.account_of(user)`.
pub(crate) const PRINCIPAL_ACCOUNT: &str = "pw:host/principal#account";

/// **The platform's account operation for one request** (track
/// `data-path`): asked with the reader's own handle, the account the
/// session's principal signed in to (its user's id and the name its provider
/// gave it), and `None` where it signed in to none: no one, or the session's
/// guest, whose user's id is `u-` and its session's (the cookie's value), a
/// credential no program is handed. A program holds no other user's handle
/// (ADR-0263), so a handle that is not the reader's is a refusal, never
/// another user's account.
pub(crate) fn account_operation(principals: &Principals, session: &str) -> HostFn {
    let user = user_of(principals, session);
    let account = principals.of(session).filter(|p| !p.is_guest()).map(|p| {
        Box::new(Val::Record(vec![
            ("id".into(), Val::String(p.user)),
            ("name".into(), Val::String(p.name)),
        ]))
    });
    Arc::new(move |args: &[Val]| match args {
        [Val::String(of)] if *of == user => Ok(vec![Val::Option(account.clone())]),
        [Val::String(_)] => {
            Err("`pw:host/principal#account` answers the reader's own handle alone".into())
        }
        other => Err(format!("principal#account received {other:?}")),
    })
}

/// **Whether a binding's argument, as a page's plan writes it, is the
/// reader's user**: `current_user()`, or `context.current_user()`.
pub(crate) fn is_current_user(arg: &str) -> bool {
    matches!(arg, "current_user()" | "context.current_user()")
}

// ---------------------------------------------------------------------------
// Notifications: a row a like, a reply or a follow writes
// ---------------------------------------------------------------------------

/// **What someone did that involves a user**, the program's `Act`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Act {
    /// Liked a post of theirs.
    Liked,
    /// Replied to a post of theirs.
    Replied,
    /// Followed them.
    Followed,
}

impl Act {
    /// The program's case, as the component names it.
    fn case(self) -> &'static str {
        match self {
            Act::Liked => "liked-your-post",
            Act::Replied => "replied-to-your-post",
            Act::Followed => "followed-you",
        }
    }

    /// As PostgreSQL keeps it (`notifications.act`).
    pub(crate) fn named(word: &str) -> Option<Act> {
        match word {
            "liked" => Some(Act::Liked),
            "replied" => Some(Act::Replied),
            "followed" => Some(Act::Followed),
            _ => None,
        }
    }
}

/// **One notification, as kept**: whom it is for, who did what, the post
/// it is about (the post liked, or the reply written; none for a follow),
/// and whether its user has read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Note {
    pub id: String,
    pub recipient: String,
    pub actor: String,
    pub act: Act,
    pub post: Option<String>,
    pub read: bool,
}

/// **The notification an act writes**, or none: one's own act notifies no
/// one. Its id is the layer's to give.
pub(crate) fn noted(recipient: &str, actor: &str, act: Act, post: Option<&str>) -> Option<Note> {
    (recipient != actor && !recipient.is_empty()).then(|| Note {
        id: String::new(),
        recipient: recipient.to_string(),
        actor: actor.to_string(),
        act,
        post: post.map(str::to_string),
        read: false,
    })
}

/// **A notification as the program's `Notification` is**: its id, who did
/// it (`actor`, the user record the layer reads for them), what, the post
/// it is about as a list of none or one, and whether it is unread.
pub(crate) fn note_val(note: &Note, actor: Val) -> Val {
    Val::Record(vec![
        ("id".into(), Val::String(note.id.clone())),
        ("actor".into(), actor),
        ("act".into(), Val::Variant(note.act.case().into(), None)),
        (
            "posts".into(),
            Val::List(note.post.iter().cloned().map(Val::String).collect()),
        ),
        ("unread".into(), Val::Bool(!note.read)),
    ])
}

/// **A user's notifications in memory**: theirs alone, newest first, at
/// most `limit`.
pub(crate) fn listed<'a>(notes: &'a [Note], reader: &str, limit: i64) -> Vec<&'a Note> {
    notes
        .iter()
        .rev()
        .filter(|n| n.recipient == reader)
        .take(limit.max(0) as usize)
        .collect()
}

/// **How many of a user's notifications are unread.**
pub(crate) fn unread(notes: &[Note], reader: &str) -> i64 {
    notes
        .iter()
        .filter(|n| n.recipient == reader && !n.read)
        .count() as i64
}

/// **Notifications in PostgreSQL** (migration `0006_notifications`): each
/// written by the statement after the act's own, in the act's transaction,
/// and read in one statement.
pub(crate) mod pg {
    use super::*;
    use postgres::Client;

    /// **A user's notifications**, newest first, at most `limit`, each with
    /// who did it as the feed shows them.
    pub(crate) fn list(c: &mut Client, reader: &str, limit: i64) -> Result<Val, postgres::Error> {
        let rows = c.query(
            "SELECT n.id, n.actor, u.handle, u.name, n.act, n.post, n.read \
             FROM notifications n LEFT JOIN users u ON u.id = n.actor \
             WHERE n.recipient = $1 ORDER BY n.seq DESC LIMIT $2",
            &[&reader, &limit.max(0)],
        )?;
        Ok(Val::List(
            rows.iter()
                .filter_map(|r| {
                    let (handle, name): (Option<String>, Option<String>) = (r.get(2), r.get(3));
                    let note = Note {
                        id: r.get(0),
                        recipient: reader.to_string(),
                        actor: r.get(1),
                        act: Act::named(r.get(4))?,
                        post: r.get(5),
                        read: r.get(6),
                    };
                    let actor = crate::feed::user_record(&note.actor, handle.zip(name));
                    Some(note_val(&note, actor))
                })
                .collect(),
        ))
    }

    /// **How many of a user's notifications are unread.**
    pub(crate) fn unread(c: &mut Client, reader: &str) -> Result<i64, postgres::Error> {
        Ok(c.query_one(
            "SELECT count(*) FROM notifications WHERE recipient = $1 AND NOT read",
            &[&reader],
        )?
        .get(0))
    }

    /// **A like's notification**, for the post's author, unless the liker
    /// is.
    pub(crate) fn liked(c: &mut Client, post: &str, liker: &str) -> Result<u64, postgres::Error> {
        c.execute(
            "INSERT INTO notifications (recipient, actor, act, post) \
             SELECT author, $2, 'liked', id FROM posts WHERE id = $1 AND author <> $2",
            &[&post, &liker],
        )
    }

    /// **A reply's notification**, for the replied-to post's author, about
    /// the reply, unless the replier is.
    pub(crate) fn replied(
        c: &mut Client,
        reply: &str,
        replier: &str,
    ) -> Result<u64, postgres::Error> {
        c.execute(
            "INSERT INTO notifications (recipient, actor, act, post) \
             SELECT t.author, $2, 'replied', r.id \
             FROM posts r JOIN posts t ON t.id = r.reply_to \
             WHERE r.id = $1 AND t.author <> $2",
            &[&reply, &replier],
        )
    }

    /// **A follow's notification**, for the one followed.
    pub(crate) fn followed(
        c: &mut Client,
        followee: &str,
        follower: &str,
    ) -> Result<u64, postgres::Error> {
        c.execute(
            "INSERT INTO notifications (recipient, actor, act) \
             SELECT $1, $2, 'followed' WHERE $1 <> $2",
            &[&followee, &follower],
        )
    }

    /// **Every notification of a user, read**: how many were unread.
    pub(crate) fn read_all(c: &mut Client, reader: &str) -> Result<u64, postgres::Error> {
        c.execute(
            "UPDATE notifications SET read = true WHERE recipient = $1 AND NOT read",
            &[&reader],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_principal_operation_answers_the_sessions_user_and_a_guests_where_none_signed_in() {
        let principals = Principals::default();
        let read = principal_operation(&principals, "s-1");
        // The guest model: every session its own guest.
        assert_eq!(
            read(&[]).expect("read"),
            vec![Val::String("u-s-1".to_string())]
        );
        // Two sessions, two users: one session's handle is never another's.
        let other = principal_operation(&principals, "s-2");
        assert_ne!(read(&[]).expect("read"), other(&[]).expect("read"));
    }

    /// **The account operation answers the reader's account, for the
    /// reader's own handle alone** (track `data-path`): its user's id and its
    /// provider's name for an account; none where no one signed in, and none
    /// for a guest, whose user's id holds its session (a credential, never
    /// handed a program); a refusal for a handle that is not the reader's.
    #[test]
    fn the_account_operation_answers_the_readers_account_for_their_handle_alone() {
        let identity = crate::identity::Identity::default();
        let principals = identity.principals();
        let account = |session: &str| account_operation(&principals, session);
        let given = |user: &str| [Val::String(user.to_string())];
        // The guest model: every session its own guest, and none an account.
        let guest = account("s-1")(&given("u-s-1"));
        assert_eq!(guest, Ok(vec![Val::Option(None)]));
        assert!(
            !format!("{guest:?}").contains("s-1"),
            "no guest's session reaches a program: {guest:?}"
        );
        identity.signed_in_for_test("s-2", "u-ada", "Ada");
        assert_eq!(
            account("s-2")(&given("u-ada")),
            Ok(vec![Val::Option(Some(Box::new(Val::Record(vec![
                ("id".into(), Val::String("u-ada".into())),
                ("name".into(), Val::String("Ada".into())),
            ]))))])
        );
        // Signed out, in the accounts model: no one.
        assert_eq!(account("s-3")(&given("u-s-3")), Ok(vec![Val::Option(None)]));
        // Another user's handle is no handle a program holds.
        let refused = account("s-3")(&given("u-ada"));
        assert!(
            refused
                .as_ref()
                .is_err_and(|why| why.contains("reader's own handle")),
            "{refused:?}"
        );
    }

    #[test]
    fn ones_own_act_notifies_no_one() {
        assert_eq!(noted("u-a", "u-a", Act::Liked, Some("p1")), None);
        assert_eq!(noted("", "u-a", Act::Liked, Some("p1")), None);
        let n = noted("u-a", "u-b", Act::Replied, Some("p4")).expect("noted");
        assert_eq!((n.recipient.as_str(), n.actor.as_str()), ("u-a", "u-b"));
        assert!(!n.read);
    }

    #[test]
    fn a_users_notifications_are_theirs_alone_newest_first() {
        let note = |id: &str, to: &str, read: bool| Note {
            id: id.into(),
            recipient: to.into(),
            actor: "u-z".into(),
            act: Act::Followed,
            post: None,
            read,
        };
        let notes = [
            note("n1", "u-a", true),
            note("n2", "u-b", false),
            note("n3", "u-a", false),
        ];
        let ids: Vec<&str> = listed(&notes, "u-a", 10)
            .iter()
            .map(|n| n.id.as_str())
            .collect();
        assert_eq!(ids, ["n3", "n1"]);
        assert_eq!(listed(&notes, "u-a", 1).len(), 1);
        assert_eq!((unread(&notes, "u-a"), unread(&notes, "u-b")), (1, 1));
        assert_eq!(unread(&notes, "u-c"), 0);
    }

    #[test]
    fn a_plans_argument_is_the_reader_by_its_call_alone() {
        assert!(is_current_user("current_user()"));
        assert!(is_current_user("context.current_user()"));
        assert!(!is_current_user("current_session()"));
        assert!(!is_current_user("user"));
    }
}
