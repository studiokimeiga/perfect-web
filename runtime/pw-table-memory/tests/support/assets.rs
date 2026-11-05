
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
