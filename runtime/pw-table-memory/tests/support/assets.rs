
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
