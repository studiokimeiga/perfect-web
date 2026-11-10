//! **What a page this host serves may load and run** (ADR-0333; ADR-0195's
//! ruling 13): every HTML answer carries a Content-Security-Policy, generated
//! from the build it serves and the deployment it runs in, never written by
//! hand.
//!
//! A document runs one inline script, the classic one that imports the
//! runtime (ADR-0148), allowed by its hash; the runtime's modules, its
//! handlers' and its WebAssembly, from this origin; its data layer's style,
//! by its hash. What the build's markup writes adds to that: a `style`
//! attribute, a `<style>` element's text, a frame. What the deployment adds:
//! its payment provider's page, which the order page frames (ADR-0332), and
//! its identity provider, where a sign-in leaves this origin (ADR-0258).
//! Every other HTML answer, a page not found, a page moved, a page the build
//! rendered with no runtime, the identity's own pages, runs nothing.

use std::collections::BTreeSet;

use base64::Engine as _;
use pw_render::ir::{Chunk, Part, Template};
use sha2::{Digest, Sha256};

/// **A hash source** (CSP Level 3, §2.3.1): `'sha256-…'`, the SHA-256 of an
/// inline element's text as UTF-8, in base64. The text is the element's as
/// the browser holds it: HTML's parser makes each CR LF, and each CR alone,
/// one LF before anything reads it (HTML, "Preprocessing the input stream"),
/// so the same is done here.
pub(crate) fn sha256_source(text: &str) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let digest = Sha256::digest(text.as_bytes());
    format!(
        "'sha256-{}'",
        base64::engine::general_purpose::STANDARD.encode(&digest[..])
    )
}

/// **The policy of an HTML answer that runs nothing**: no script, no style
/// but a stylesheet of this origin's, images of this origin's, forms sent
/// here, and framed by no page.
pub(crate) const BARE: &str = "default-src 'none'; img-src 'self'; style-src 'self'; \
     form-action 'self'; frame-ancestors 'none'; base-uri 'none'";

/// **What a build's markup asks of its pages' policy**, read from its
/// templates as the renderer writes them: once for the whole build, since
/// a navigation that keeps its layout (ADR-0306) keeps its first page's
/// document, and with it that page's policy.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Needs {
    /// A `style` attribute, written or a value's, or markup a capability
    /// lets through whole (`Part::RawHtml`), which may hold one.
    pub(crate) style_attributes: bool,
    /// Each `<style>` element's text, which only text may be (PW5023's
    /// neighbour in `check.rs`): each allowed by its hash.
    pub(crate) styles: BTreeSet<String>,
    /// An `<iframe>`: the page frames the deployment's payment provider.
    pub(crate) frames: bool,
    /// A `data:` image in one of the build's own stylesheets.
    pub(crate) data_images: bool,
}

impl Needs {
    pub(crate) fn of(templates: &[Template]) -> Needs {
        let mut needs = Needs::default();
        for t in templates {
            needs.chunks(&t.chunks);
        }
        needs
    }

    /// One run of chunks: its static markup read as one, each part's own
    /// markup in turn. A part's chunks are its block's, which starts and ends
    /// between elements, never inside a tag.
    fn chunks(&mut self, chunks: &[Chunk]) {
        let mut markup = String::new();
        for c in chunks {
            match c {
                Chunk::Static(s) => markup.push_str(s),
                Chunk::Dynamic(p) => {
                    match p {
                        Part::Attribute { name, .. }
                        | Part::BooleanAttribute { name, .. }
                        | Part::InterpolatedAttribute { name, .. }
                            if name.eq_ignore_ascii_case("style") =>
                        {
                            self.style_attributes = true;
                        }
                        Part::RawHtml { .. } => self.style_attributes = true,
                        _ => {}
                    }
                    for inner in p.nested() {
                        self.chunks(inner);
                    }
                }
            }
        }
        self.markup(&markup);
    }

    /// The tags of markup the compiler wrote, where every `<` is a tag's: it
    /// writes a text's as `&lt;` (ADR-0094), and quotes each attribute's
    /// value with `"`, which a value writes as `&quot;`.
    fn markup(&mut self, html: &str) {
        let mut rest = html;
        while let Some(open) = rest.find('<') {
            rest = &rest[open + 1..];
            if let Some(comment) = rest.strip_prefix("!--") {
                rest = comment.find("-->").map_or("", |end| &comment[end + 3..]);
                continue;
            }
            if rest.starts_with('/') {
                continue;
            }
            let name_end = rest
                .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
                .unwrap_or(rest.len());
            let name = rest[..name_end].to_ascii_lowercase();
            rest = &rest[name_end..];
            // Its attributes, to the tag's end.
            loop {
                rest = rest.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '/');
                if rest.is_empty() {
                    break;
                }
                if let Some(after) = rest.strip_prefix('>') {
                    rest = after;
                    break;
                }
                let attribute_end = rest
                    .find(|c: char| c.is_ascii_whitespace() || matches!(c, '=' | '>' | '/'))
                    .unwrap_or(rest.len());
                if rest[..attribute_end].eq_ignore_ascii_case("style") {
                    self.style_attributes = true;
                }
                rest = &rest[attribute_end..];
                if let Some(value) = rest.strip_prefix('=') {
                    rest = match value.chars().next() {
                        Some(quote @ ('"' | '\'')) => {
                            let quoted = &value[1..];
                            quoted.find(quote).map_or("", |end| &quoted[end + 1..])
                        }
                        _ => {
                            let end = value
                                .find(|c: char| c.is_ascii_whitespace() || c == '>')
                                .unwrap_or(value.len());
                            &value[end..]
                        }
                    };
                }
            }
            match name.as_str() {
                "iframe" => self.frames = true,
                // A raw text element: its text runs to its end tag, in any
                // case (HTML, "Parsing HTML documents", RAWTEXT).
                "style" => {
                    let end = rest
                        .to_ascii_lowercase()
                        .find("</style")
                        .unwrap_or(rest.len());
                    self.styles.insert(rest[..end].to_string());
                    rest = &rest[end..];
                }
                _ => {}
            }
        }
    }
}

/// **What the deployment adds**: an origin each page may frame, and one a
/// sign-in form's answer may send the browser to.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Deployment {
    /// The payment provider's origin, whose page the order page frames
    /// (ADR-0332): none where the deployment takes no payments.
    pub(crate) frames: Option<String>,
    /// The identity provider's origin, where it is not this one: a form
    /// that starts a sign-in (`GET /sign-in`) is answered with a redirect
    /// there, and Chrome holds a form's redirect to `form-action` too.
    /// Whether a browser should is debated, and engines differ (MDN's
    /// `form-action`, read 2026-10-14); CSP Level 3's own check (§6.4.1.1)
    /// names no redirect.
    pub(crate) forms: Option<String>,
}

/// **A document's policy**: `script` is the inline script's text that starts
/// the runtime, `style` the data layer's stylesheet (none where it has
/// none), `needs` what the build's markup asks and `deployment` what the
/// deployment adds.
#[cfg(test)]
pub(crate) fn document(
    script: &str,
    style: &str,
    needs: &Needs,
    deployment: &Deployment,
) -> String {
    document_with(&[script], style, needs, deployment)
}

/// [`document`], allowing each of `scripts`: a build whose pages start one of
/// two runtimes (ADR-0413).
pub(crate) fn document_with(
    scripts: &[&str],
    style: &str,
    needs: &Needs,
    deployment: &Deployment,
) -> String {
    let mut styles = vec!["'self'".to_string()];
    if !style.is_empty() {
        styles.push(sha256_source(style));
    }
    styles.extend(needs.styles.iter().map(|s| sha256_source(s)));
    let mut directives = vec![
        "default-src 'none'".to_string(),
        // The runtime and its modules from this origin, and nothing compiled
        // on the page: there is no WebAssembly to allow.
        format!(
            "script-src 'self' {}",
            scripts
                .iter()
                .map(|s| sha256_source(s))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        format!("style-src {}", styles.join(" ")),
    ];
    // A style attribute the build writes. `'unsafe-inline'` alone: beside a
    // hash or a nonce it allows nothing (CSP Level 3, §6.7.3.2).
    if needs.style_attributes {
        directives.push("style-src-attr 'unsafe-inline'".to_string());
    }
    directives.push(
        if needs.data_images {
            "img-src 'self' data:"
        } else {
            "img-src 'self'"
        }
        .to_string(),
    );
    directives.push("connect-src 'self'".to_string());
    if let (true, Some(provider)) = (needs.frames, &deployment.frames) {
        directives.push(format!("frame-src {provider}"));
    }
    directives.push(match &deployment.forms {
        Some(provider) => format!("form-action 'self' {provider}"),
        None => "form-action 'self'".to_string(),
    });
    directives.push("frame-ancestors 'none'".to_string());
    directives.push("base-uri 'none'".to_string());
    directives.join("; ")
}

/// **The origin of an absolute address**, `scheme://host[:port]`, as a CSP
/// source expression writes one; `None` for a path on this origin.
pub(crate) fn origin_of(address: &str) -> Option<String> {
    let (scheme, rest) = address.split_once("://")?;
    if scheme.is_empty()
        || !scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
    {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    // Who it is from is no part of where it is.
    let host = authority.rsplit('@').next().unwrap_or_default();
    (!host.is_empty()).then(|| {
        format!(
            "{}://{}",
            scheme.to_ascii_lowercase(),
            host.to_ascii_lowercase()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(chunks: Vec<Chunk>) -> Template {
        Template {
            path: "t.T".into(),
            name: "T".into(),
            params: Vec::new(),
            schema: "s".into(),
            chunks,
        }
    }

    fn stat(s: &str) -> Chunk {
        Chunk::Static(s.to_string())
    }

    /// **A hash is CSP's**: the SHA-256 of the text in standard base64,
    /// quoted. The digest of nothing is FIPS 180-4's, `e3b0c442…b855`.
    #[test]
    fn a_hash_source_is_the_texts_digest_in_base64() {
        assert_eq!(
            sha256_source(""),
            "'sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU='"
        );
        // A line's end as the parser leaves it.
        assert_eq!(sha256_source("a\r\nb\rc"), sha256_source("a\nb\nc"));
        assert_ne!(sha256_source("a"), sha256_source("a "));
    }

    /// **Nothing in markup that asks for nothing**: tags, attributes, a
    /// comment, text with `style=` in it, a value holding `<iframe`.
    #[test]
    fn plain_markup_asks_nothing() {
        let needs = Needs::of(&[template(vec![stat(
            "<main><!-- <iframe> --><p class=\"a\" title=\"&lt;iframe style=x\">style=\"x\" \
             &lt;iframe&gt;</p><a href=\"/x\">x</a><img src=\"/logo.png\" alt=\"\"></main>",
        )])]);
        assert_eq!(needs, Needs::default());
    }

    /// **A comment's text is no markup, whatever it says**: a `>` in it
    /// does not end it, and a tag after one is not the page's. Train-6's run
    /// found the mutant that reads a comment's text as markup surviving:
    /// each comment the tests wrote held its tag first, which the mutant
    /// took as the comment's own attributes.
    #[test]
    fn a_comment_holds_no_markup_whatever_it_says() {
        let needs = Needs::of(&[template(vec![stat(
            "<main><!-- when a > b, show <iframe src=\"/x\"> and <p style=\"x\"> --><p>a</p></main>",
        )])]);
        assert_eq!(needs, Needs::default());
    }

    /// **A style attribute, written or a value's, in any case and at any
    /// depth**; a frame; and a `<style>` element's text, to its end tag.
    #[test]
    fn what_markup_writes_is_found_wherever_it_is() {
        let written = Needs::of(&[template(vec![stat("<p STYLE=\"color:red\">a</p>")])]);
        assert!(written.style_attributes);
        let unquoted = Needs::of(&[template(vec![stat("<p id=a style=x>a</p>")])]);
        assert!(unquoted.style_attributes);
        let valued = Needs::of(&[template(vec![
            stat("<p data-pw=\"0\" "),
            Chunk::Dynamic(Part::Attribute {
                id: pw_render::ir::PartId(0),
                owner: pw_render::ir::ElementId(0),
                name: "style".into(),
                value: "s".into(),
                context: pw_render::ir::Context::Style,
            }),
            stat(">a</p>"),
        ])]);
        assert!(valued.style_attributes);
        let nested = Needs::of(&[template(vec![
            stat("<div>"),
            Chunk::Dynamic(Part::Conditional {
                id: pw_render::ir::PartId(1),
                value: "c".into(),
                then: vec![stat("<iframe src=\"/x\" title=\"t\"></iframe>")],
                otherwise: vec![stat("<style>@import \"/a.css\";</STYLE>")],
            }),
            stat("</div>"),
        ])]);
        assert!(nested.frames);
        assert!(!nested.style_attributes);
        assert_eq!(
            nested.styles.iter().collect::<Vec<_>>(),
            vec!["@import \"/a.css\";"]
        );
    }

    /// **Markup a capability lets through whole may hold a style attribute**
    /// (`Part::RawHtml`), which nothing reads before it is rendered.
    #[test]
    fn raw_markup_may_hold_a_style_attribute() {
        let raw = Needs::of(&[template(vec![Chunk::Dynamic(Part::RawHtml {
            id: pw_render::ir::PartId(0),
            value: "v".into(),
            capability: "unsafe.raw_html".into(),
        })])]);
        assert!(raw.style_attributes);
    }

    /// **A document's policy**: the script by its hash, the style by its
    /// hash, and nothing the build and the deployment do not ask.
    #[test]
    fn a_documents_policy_allows_what_it_is_given_and_no_more() {
        let none = document("go()", "", &Needs::default(), &Deployment::default());
        assert_eq!(
            none,
            format!(
                "default-src 'none'; script-src 'self' {}; style-src 'self'; \
                 img-src 'self'; connect-src 'self'; form-action 'self'; \
                 frame-ancestors 'none'; base-uri 'none'",
                sha256_source("go()")
            )
        );
        let needs = Needs {
            style_attributes: true,
            styles: ["@import \"/a.css\";".to_string()].into(),
            data_images: false,
            frames: true,
        };
        let deployment = Deployment {
            frames: Some("http://127.0.0.1:3401".into()),
            forms: Some("https://id.example".into()),
        };
        let all = document("go()", "p{}", &needs, &deployment);
        assert!(
            all.contains(&format!(
                "style-src 'self' {} {}",
                sha256_source("p{}"),
                sha256_source("@import \"/a.css\";")
            )),
            "{all}"
        );
        assert!(all.contains("; style-src-attr 'unsafe-inline';"), "{all}");
        assert!(all.contains("; frame-src http://127.0.0.1:3401;"), "{all}");
        assert!(
            all.contains("; form-action 'self' https://id.example;"),
            "{all}"
        );
        // A deployment's provider, framed by a build that frames nothing: not.
        let unframed = document("go()", "", &Needs::default(), &deployment);
        assert!(!unframed.contains("frame-src"), "{unframed}");
    }

    /// **An address's origin**: its scheme and authority, without who it is
    /// from; a path has none.
    #[test]
    fn an_origin_is_a_scheme_and_an_authority() {
        assert_eq!(
            origin_of("https://ID.example:8443/authorize?x=1").as_deref(),
            Some("https://id.example:8443")
        );
        assert_eq!(
            origin_of("https://user:pw@id.example/a").as_deref(),
            Some("https://id.example")
        );
        assert_eq!(origin_of("/dev-idp/authorize"), None);
        assert_eq!(origin_of("https:///nothing"), None);
    }
}
