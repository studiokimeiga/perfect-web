
// The same compiled public asset declaration is reused at both adapters.
fn asset_program()->Program {
    built_from(&[("PostImage.pw",r#"module PostImage
upload PostImage
    route "/uploads/post-image"
    serves "/images"
    max_bytes 5_000_000
    types png, jpeg
    max_width 4096
    max_height 4096
"#),("Assets.pw",r#"module Assets
import PostImage
type Item = Item { id: Int, image: Option<PostImage.Ref>, alt: String }
table Assets
    row Item
    key id
    bounded 100
    asset image from PostImage
"#),("AssetData.pw",r#"module AssetData
import PostImage
import Assets
source AssetData
    holds PostImage, Assets
    transactions serializable
    reads strong
    changes none
    sets one_call
"#),("app.pw",r#"module owned.app
import PostImage
import context.{ current_session }
import capability.{ Session, SessionId }
opaque type InteractionId = String
command Keep() -> Result<PostImage.Ref, PostImage.Error>
    requires Anyone
    idempotent_by InteractionId
    invalidates Attached(current_session())
    transaction serializable
{
    let claim = PostImage.claim(current_session())?
    PostImage.publish(current_session(),claim)
}
session query Attached(session: Session<SessionId>) -> List<PostImage.Attached>
    freshness 0.seconds
    consistency read_your_writes
    cache private
    key session
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{
    PostImage.attached(session)
}
public query Url(image: PostImage.Ref) -> String
    freshness 0.seconds
    consistency snapshot
    cache shared
    key image
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{
    PostImage.src(image)
}
"#)])
}
/// **The asset program beside a table that keeps no asset** (L7's G10,
/// ADR-0440 amended 2026-11-17): `Keep` claims and publishes, as in
/// [`asset_program`]; `Forget` deletes an asset's row, claiming nothing;
/// `Note` writes the notes alone. What each works on is its imports'.
fn asset_work_program()->Program {
    built_from(&[("PostImage.pw",r#"module PostImage
upload PostImage
    route "/uploads/post-image"
    serves "/images"
    max_bytes 5_000_000
    types png, jpeg
    max_width 4096
    max_height 4096
"#),("Assets.pw",r#"module Assets
import PostImage
type Item = Item { id: Int, image: Option<PostImage.Ref>, alt: String }
table Assets
    row Item
    key id
    bounded 100
    asset image from PostImage
    written by anyone
"#),("Notes.pw",r#"module Notes
type Note = Note { id: Int, text: String }
table Notes
    row Note
    key id
    written by anyone
"#),("AssetData.pw",r#"module AssetData
import PostImage
import Assets
import Notes
source AssetData
    holds PostImage, Assets, Notes
    transactions serializable
    reads strong
    changes none
    sets one_call
"#),("app.pw",r#"module owned.app
import PostImage
import Assets
import Notes
import context.{ current_session }
import capability.{ Session, SessionId }
opaque type InteractionId = String
command Keep() -> Result<PostImage.Ref, PostImage.Error>
    requires Anyone
    idempotent_by InteractionId
    invalidates Attached(current_session())
    transaction serializable
{
    let claim = PostImage.claim(current_session())?
    PostImage.publish(current_session(),claim)
}
command Forget(id: Int) -> ()
    requires Anyone
    transaction serializable
{ Assets.delete(id) }
command Note(note: Notes.Note) -> ()
    requires Anyone
    transaction serializable
{ Notes.put(note) }
session query Attached(session: Session<SessionId>) -> List<PostImage.Attached>
    freshness 0.seconds
    consistency read_your_writes
    cache private
    key session
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{
    PostImage.attached(session)
}
"#)])
}

/// [`asset_work_program`]'s notes in a program that declares no upload:
/// what an ordinary command costs where nothing is uploaded (L7's G10).
fn notes_alone_program()->Program {
    built_from(&[("Notes.pw",r#"module Notes
type Note = Note { id: Int, text: String }
table Notes
    row Note
    key id
    written by anyone
"#),("NoteData.pw",r#"module NoteData
import Notes
source NoteData
    holds Notes
    transactions serializable
    reads strong
    changes none
    sets one_call
"#),("app.pw",r#"module owned.app
import Notes
command Note(note: Notes.Note) -> ()
    requires Anyone
    transaction serializable
{ Notes.put(note) }
"#)])
}

/// **Posts owned by their author, keeping an image** (L7's G4, ADR-0440
/// amended 2026-11-17): the feed's shape, a `public table` its author alone
/// writes and every reader reads (ADR-0395's ruling 4). `Publish` claims and
/// publishes the image the reader attached, into the reader's own post;
/// `Keep` puts a post given whole, as a forged request would give it, the
/// reader's handle beside it; `TakeDown` deletes the reader's own post of
/// an id. Each writes a table keeping an asset, and works on the uploads.
fn owned_asset_program()->Program {
    built_from(&[("PostImage.pw",r#"module PostImage
upload PostImage
    route "/uploads/post-image"
    serves "/images"
    max_bytes 5_000_000
    types png, jpeg
    max_width 4096
    max_height 4096
"#),("Posts.pw",r#"module Posts
import PostImage
import capability.{ UserId }
type Post = Post { author: UserId, id: Int, image: Option<PostImage.Ref>, alt: String }
public table Posts
    row Post
    key author, id
    bounded 100
    owned by author
    asset image from PostImage
"#),("AssetData.pw",r#"module AssetData
import PostImage
import Posts
source AssetData
    holds PostImage, Posts
    transactions serializable
    reads strong
    changes none
    sets one_call
"#),("app.pw",r#"module owned.app
import PostImage
import Posts
import Posts.{ Post }
import context.{ current_session, current_user }
import capability.{ Session, SessionId, id_of }
opaque type InteractionId = String
command Publish(id: Int, alt: String) -> Result<Int, PostImage.Error>
    requires Anyone
    idempotent_by InteractionId
    invalidates Attached(current_session())
    transaction serializable
{
    let me = current_user()
    let claim = PostImage.claim(current_session())?
    let image = PostImage.publish(current_session(),claim)?
    let saved = Posts.put(me, Post { author: id_of(me), id: id, image: Some(image), alt: alt })
    Ok(id)
}
command Keep(post: Post) -> ()
    requires Anyone
    transaction serializable
{ Posts.put(current_user(), post) }
command TakeDown(id: Int) -> ()
    requires Anyone
    transaction serializable
{ Posts.delete(current_user(), id) }
session query Attached(session: Session<SessionId>) -> List<PostImage.Attached>
    freshness 0.seconds
    consistency read_your_writes
    cache private
    key session
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{
    PostImage.attached(session)
}
"#)])
}

/// A post of [`owned_asset_program`] as the adapter stores it.
fn owned_post(author: &str, id: i64, image: Option<&pw_host::assets::Reference>) -> serde_json::Value {
    json!({"author": author, "id": id, "image": image, "alt": format!("{author}'s post {id}")})
}

fn image_reference()->Val {
    Val::Record(vec![("key".into(),Val::String("a".repeat(64))),
        ("generation".into(),Val::String("b".repeat(32))),
        ("kind".into(),Val::Variant("png".into(),None)),
        ("width".into(),Val::S64(6)),("height".into(),Val::S64(4))])
}

fn upload_hooks(component:&Component,calls:&Arc<AtomicUsize>)->BTreeMap<String,HostFn>{
    component.contract.imports.iter().map(|import|{
        let key=import.key();let called=Arc::clone(calls);
        let (upload,answer)=match (import.interface.as_str(),import.name.as_str()) {
            ("pw:host/session","read")=>(false,vec![Val::String("request-session".into())]),
            ("pw:uploads/post-image","claim")=>(true,vec![Val::Result(Ok(Some(Box::new(Val::String("actual-claim-token".into())))))]),
            ("pw:uploads/post-image","publish")=>(true,vec![Val::Result(Ok(Some(Box::new(image_reference()))))]),
            ("pw:uploads/post-image","attached")=>(true,vec![Val::List(vec![Val::Record(vec![("src".into(),Val::String("/uploads/post-image/lease-token".into())),("width".into(),Val::S64(6)),("height".into(),Val::S64(4))])])]),
            _ if import.invalidates.is_some()=>(false,vec![]),
            _=>panic!("upload fixture supplied no meaning for {key}"),
        };
        (key,Arc::new(move|_:&[Val]|{if upload{called.fetch_add(1,Ordering::SeqCst);}Ok(answer.clone())}) as HostFn)
    }).collect()
}
