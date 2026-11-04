// One standalone compiled relation program, reused by both generic adapters.
fn relation_program() -> Program {
    let roots=r#"module Roots
import capability.{ UserId }
type Root = Root { id: Int, author: UserId, stamp: Int, text: String }
table Roots
    row Root
    key id
    order stamp
    index by_author on author
"#;
    let links=r#"module Links
import Roots
import capability.{ UserId }
type Link = Link { reader: UserId, topic: Int, author: UserId, edge: Int, to: UserId, secret: String }
table Links
    row Link
    key reader, topic, author, edge
    order edge
    index by_recipient_topic_author on to, topic, author
    owned by reader
    shares with to
    private secret
    window posts from Roots row Roots.Root on author to author
"#;
    let data=r#"module RelationData
import Roots
import Links
source RelationData
    holds Roots, Links
    transactions serializable
    reads strong
    changes none
    sets one_call
"#;
    let mut app="module owned.app\nimport List\nimport Roots\nimport Links\nimport capability.{ User, UserId }\n".to_string();
    for (name,returns,body) in [
        ("RelFirst","List<Roots.Root>","Links.first_posts_by_reader_topic(reader,topic,n)"),
        ("RelLast","List<Roots.Root>","Links.last_posts_by_reader_topic(reader,topic,n)"),
        ("RelShared","List<Roots.Root>","Links.first_posts_by_to_topic(reader,topic,n)"),
    ] {
        app.push_str(&format!("\nuser query {name}(reader: User<UserId>, topic: Int, n: Int) -> {returns}\n    freshness 0.seconds\n    consistency read_your_writes\n    cache private\n    key reader,topic,n\n    concurrency one_per_key\n    on_key_change cancel\n    timeout 2.seconds\n{{\n    {body}\n}}\n"));
    }
    for (name,returns,body) in [
        ("RelMany","List<Links.LinksReaderTopicPostsWindow>","Links.last_posts_by_reader_topic_many(reader,topics,n)"),
        ("RelSharedMany","List<Links.LinksToTopicPostsWindow>","Links.first_posts_by_to_topic_many(reader,topics,n)"),
    ] {
        app.push_str(&format!("\nuser query {name}(reader: User<UserId>, topics: List<Int>, n: Int) -> {returns}\n    freshness 0.seconds\n    consistency read_your_writes\n    cache private\n    key reader,topics,n\n    concurrency one_per_key\n    on_key_change cancel\n    timeout 2.seconds\n{{\n    {body}\n}}\n"));
    }
    app.push_str(r#"
import context.{ current_user }
predicate Selected(topic: Int, id: Int)
    says "This post is no longer selected."
{
    List.any(Links.last_posts_by_reader_topic(current_user(),topic,1),post => post.id == id)
}
command RelGuarded(topic: Int, id: Int) -> Option<Roots.Root>
    requires Selected(topic,id)
    transaction serializable
{
    Roots.get(id)
}
user query RelHeld(reader: User<UserId>, topic: Int, id: Int) -> Option<Roots.Root>
    requires Selected(topic,id)
    freshness 0.seconds
    consistency read_your_writes
    cache private
    key reader,topic,id
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{
    Roots.get(id)
}
"#);
    let files=[("Roots.pw",roots),("Links.pw",links),("RelationData.pw",data),("app.pw",&app)];
    built_from(&files)
}

fn relation_roots() -> Vec<serde_json::Value> {
    [(1,BOB,1,"B1"),(2,CAROL,2,"C2"),(3,BOB,3,"B3"),(4,CAROL,3,"C3"),(5,ALICE,99,"other-reader-sentinel")]
        .into_iter().map(|(id,author,stamp,text)|json!({"id":id,"author":author,"stamp":stamp,"text":text})).collect()
}
fn relation_links() -> Vec<serde_json::Value> {
    [(ALICE,10,BOB,1,BOB),(ALICE,10,BOB,2,BOB),(ALICE,10,CAROL,3,BOB),(ALICE,20,BOB,4,BOB),
     (BOB,10,ALICE,5,CAROL),(CAROL,10,ALICE,6,ALICE)]
        .into_iter().map(|(reader,topic,author,edge,to)|json!({"reader":reader,"topic":topic,"author":author,"edge":edge,"to":to,"secret":format!("{reader} private membership metadata")})).collect()
}
fn relation_root_value(id:i64)->Val {
    let row=relation_roots().into_iter().find(|r|r["id"]==id).unwrap();
    Val::Record(vec![("id".into(),Val::S64(id)),("author".into(),Val::String(row["author"].as_str().unwrap().into())),
        ("stamp".into(),Val::S64(row["stamp"].as_i64().unwrap())),("text".into(),Val::String(row["text"].as_str().unwrap().into()))])
}
fn relation_group(reader_field:&str, reader:&str, topic:i64, ids:&[i64])->Val {
    Val::Record(vec![("key".into(),Val::Record(vec![(reader_field.into(),Val::String(reader.into())),("topic".into(),Val::S64(topic))])),
        ("rows".into(),Val::List(ids.iter().map(|id|relation_root_value(*id)).collect()))])
}


fn relation_public_program() -> Program {
    let roots=r#"module Roots
import capability.{ UserId }
type Root = Root { id: Int, author: UserId, stamp: Int, text: String }
table Roots
    row Root
    key id
    order stamp
    index by_author on author
"#;
    let links=r#"module Links
import Roots
import capability.{ UserId }
type Link = Link { topic: Bool, author: UserId, edge: Int, text: String }
table Links
    row Link
    key topic, author, edge
    order edge
    window posts from Roots row Roots.Root on author to author
"#;
    let source=r#"module RelationData
import Links
import Roots
source RelationData
    holds Links, Roots
    transactions serializable
    reads strong
    changes none
    sets one_call
"#;
    let app=r#"module owned.app
import Roots
import Links
public query PlainLast(topic: Bool, n: Int) -> List<Roots.Root>
    freshness 0.seconds
    consistency snapshot
    cache shared
    key topic,n
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{
    Links.last_posts_by_topic(topic,n)
}
public query PlainMany(topics: List<Bool>, n: Int) -> List<Links.LinksTopicPostsWindow>
    freshness 0.seconds
    consistency snapshot
    cache shared
    key topics,n
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{
    Links.last_posts_by_topic_many(topics,n)
}
"#;
    built_from(&[("Roots.pw",roots),("Links.pw",links),("RelationData.pw",source),("app.pw",app)])
}
fn relation_public_links()->Vec<serde_json::Value> {
    [(false,BOB,1),(false,BOB,2),(true,BOB,3),(true,CAROL,4)].into_iter()
        .map(|(topic,author,edge)|json!({"topic":topic,"author":author,"edge":edge,"text":"public membership"})).collect()
}
fn relation_public_group(topic:bool,ids:&[i64])->Val {
    Val::Record(vec![("key".into(),Val::Bool(topic)),("rows".into(),Val::List(ids.iter().map(|id|relation_root_value(*id)).collect()))])
}
