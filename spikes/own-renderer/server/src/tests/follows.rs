//! **The follows timeline** (ADR-0257, ADR-0195's ruling 10): following
//! and unfollowing, the timeline of those a reader follows, and a user's
//! page counting who follows them and whom they follow, on the in-memory
//! layer and on PostgreSQL. Each test states one case, with its control.

use super::*;

/// A page's parameter.
fn of(id: &str) -> Params {
    Params::from([("id".to_string(), id.to_string())])
}

/// The text of the element whose id is `id`, as the document has it.
fn text_of(html: &str, id: &str) -> String {
    let at = html.find(&format!("id=\"{id}\"")).expect("the element");
    let start = at + html[at..].find('>').expect("its tag's end") + 1;
    let end = start + html[start..].find("</").expect("its end");
    visible(&html[start..end])
}

/// What `session`'s page `page` shows, as text.
fn shown(s: &Server, session: &str, page: &str, params: &Params) -> String {
    let (html, _, _, _) = s
        .serve_document_settled(session, page, params, &[])
        .expect("served");
    visible(&html)
}

fn follow(s: &Server, session: &str, verb: &str, user: &str, interaction: &str) -> Answered {
    s.command_answered(
        &format!("feed.app.{verb}"),
        session,
        &[Val::String(user.into())],
        Some(interaction),
    )
    .expect("runs")
}

/// **Following brings their posts into the timeline of those you follow,
/// and unfollowing takes them out**; the timeline of everyone keeps them.
fn follows_bring_their_posts(s: &Server) {
    let page = |s: &Server| shown(s, "a", "feed.app.FollowingPage~ui", &Params::new());
    let before = page(s);
    assert!(
        before.contains("Follow someone to see their posts here."),
        "{before}"
    );
    assert!(!before.contains("Hello, feed."), "{before}");
    let answered = follow(s, "a", "follow", "u-ada", "i-1");
    assert!(answered.committed, "{:?}", answered.result);
    let after = page(s);
    assert!(after.contains("Hello, feed."), "{after}");
    assert!(!after.contains("Follow someone"), "{after}");
    let answered = follow(s, "a", "unfollow", "u-ada", "i-2");
    assert!(answered.committed, "{:?}", answered.result);
    assert!(!page(s).contains("Hello, feed."));
    // The control: everyone's timeline shows it throughout.
    assert!(shown(s, "a", "feed.app.Home~ui", &Params::new()).contains("Hello, feed."));
    // And the reader's own posts are in its own, followed or not.
    let answered = s
        .command_answered(
            "feed.app.post",
            "a",
            &[Val::String("Mine, in my timeline".into())],
            Some("i-3"),
        )
        .expect("runs");
    assert!(answered.committed, "{:?}", answered.result);
    assert!(page(s).contains("Mine, in my timeline"));
}

#[test]
fn following_brings_their_posts_into_your_timeline() {
    follows_bring_their_posts(&served_feed());
}

/// **A user's page counts who follows them and whom they follow**, and a
/// follow reaches another reader's open page.
fn a_page_counts_follows(s: &Server) {
    let counts = |s: &Server, session: &str, id: &str| {
        let (html, _, _, _) = s
            .serve_document_settled(session, "feed.app.ProfilePage~ui", &of(id), &[])
            .expect("served");
        text_of(&html, "follow-counts")
    };
    assert_eq!(counts(s, "b", "u-ada"), "0 followers · 0 following");
    let theirs = latest(&s.pending.lock().expect("pending"), "b");
    let answered = follow(s, "a", "follow", "u-ada", "i-1");
    assert!(answered.committed, "{:?}", answered.result);
    s.tell_waiting();
    let set = format!(
        "{:?}",
        sets_of(s, &theirs).pop().expect("b's open page is told")
    );
    assert!(set.contains("1 follower"), "{set}");
    assert_eq!(counts(s, "b", "u-ada"), "1 follower · 0 following");
    // And the follower's own: one it follows. Following again is no change.
    let answered = follow(s, "a", "follow", "u-ada", "i-2");
    assert!(answered.committed, "{:?}", answered.result);
    assert_eq!(counts(s, "b", "u-ada"), "1 follower · 0 following");
    let a = crate::identity::guest_user("a");
    assert_eq!(counts(s, "b", &a), "0 followers · 1 following");
}

#[test]
fn a_users_page_counts_who_follows_them() {
    a_page_counts_follows(&served_feed());
}

/// **What the reader is to the user decides the button**: Follow, Unfollow,
/// or none on the reader's own page.
fn the_relation_is_the_readers(s: &Server) {
    let page = |s: &Server, id: &str| shown(s, "a", "feed.app.ProfilePage~ui", &of(id));
    assert!(page(s, "u-ada").contains("Follow"));
    assert!(!page(s, "u-ada").contains("Unfollow"));
    let answered = follow(s, "a", "follow", "u-ada", "i-1");
    assert!(answered.committed, "{:?}", answered.result);
    assert!(page(s, "u-ada").contains("Unfollow"));
    // Another reader's is its own.
    let theirs = shown(s, "b", "feed.app.ProfilePage~ui", &of("u-ada"));
    assert!(!theirs.contains("Unfollow"), "{theirs}");
    // The reader's own page, once it is someone: it follows.
    let mine = page(s, &crate::identity::guest_user("a"));
    assert!(mine.contains("This is you."), "{mine}");
    assert!(!mine.contains("Follow"), "{mine}");
}

#[test]
fn the_button_is_what_the_reader_is_to_the_user() {
    the_relation_is_the_readers(&served_feed());
}

/// **Following yourself, or no one, is not found**, and nothing commits;
/// a page of no one is not found.
fn no_one_is_not_found(s: &Server) {
    // `a` is someone once it follows.
    let answered = follow(s, "a", "follow", "u-ada", "i-1");
    assert!(answered.committed, "{:?}", answered.result);
    let a = crate::identity::guest_user("a");
    for (user, interaction) in [(a.as_str(), "i-2"), ("u-nobody", "i-3")] {
        let answered = follow(s, "a", "follow", user, interaction);
        assert!(!answered.committed, "{user}: {:?}", answered.result);
        assert!(
            format!("{:?}", answered.result).contains("not-found"),
            "{user}: {:?}",
            answered.result
        );
    }
    let absent = s
        .serve_document_settled("a", "feed.app.ProfilePage~ui", &of("u-nobody"), &[])
        .expect_err("no one");
    assert!(matches!(absent, Unread::NotFound(_)), "{absent:?}");
}

#[test]
fn following_yourself_or_no_one_is_not_found() {
    no_one_is_not_found(&served_feed());
}

/// The feed on PostgreSQL, where the environment names a database.
fn on_postgres(test: &str) -> Option<super::feed_pg::OnPostgres> {
    let url = match std::env::var("PW_FEED_DATABASE_URL") {
        Ok(url) if !url.is_empty() => url,
        _ => {
            eprintln!("skipped: PW_FEED_DATABASE_URL is not set");
            return None;
        }
    };
    Some(super::feed_pg::served_on(&url, test))
}

#[test]
fn on_postgres_following_brings_their_posts_into_your_timeline() {
    let Some(s) = on_postgres("follow_timeline") else {
        return;
    };
    follows_bring_their_posts(&s);
}

#[test]
fn on_postgres_a_users_page_counts_who_follows_them() {
    let Some(s) = on_postgres("follow_counts") else {
        return;
    };
    a_page_counts_follows(&s);
}

#[test]
fn on_postgres_the_button_is_what_the_reader_is_to_the_user() {
    let Some(s) = on_postgres("follow_relation") else {
        return;
    };
    the_relation_is_the_readers(&s);
}

#[test]
fn on_postgres_following_yourself_or_no_one_is_not_found() {
    let Some(s) = on_postgres("follow_none") else {
        return;
    };
    no_one_is_not_found(&s);
    // And the table refuses it too.
    assert_eq!(
        s.db.count("SELECT count(*) FROM follows WHERE follower = followee"),
        0
    );
}
