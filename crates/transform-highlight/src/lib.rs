//! First-party syntax highlighting transformer.
//!
//! Highlights every code block whose language it knows (by name or file
//! extension, e.g. `rust` or `rs`), and wraps it in a `custom:highlight`
//! node. Highlighting uses `syntect` with the syntaxes `bat` ships (via
//! `two-face`), which include TOML, TypeScript and many more:
//!
//! ```json
//! {"type": "custom", "kind": "highlight",
//!  "attrs": {"language": "rust", "html": "<pre class=\"highlight\">...</pre>\n"},
//!  "children": [{"type": "code_block", "language": "rust", "value": "..."}]}
//! ```
//!
//! `attrs.html` is the highlighted block for HTML renderers. Tokens are
//! `<span>`s with CSS classes from their syntax scopes, prefixed `hl-`
//! (`keyword.control` becomes `hl-keyword hl-control`), so a stylesheet
//! picks the colors and light and dark themes both work; `render-html`'s
//! default theme has one. The original code block stays as the only child,
//! so renderers that don't know `custom:highlight` can render it unchanged.
//!
//! Code blocks without a language, or in a language it does not know, are
//! left alone.
//!
//! Options: `languages`, a list of language names or extensions to
//! highlight (default: all), and `aliases`, a table mapping extra names to
//! known ones, e.g. `{ jsonc = "json" }`.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use lsdocs_plugin_sdk::doctree::{CodeBlock, Custom, Node};
use lsdocs_plugin_sdk::protocol::{TransformApplyParams, TransformApplyResult};
use lsdocs_plugin_sdk::{Context, Plugin, PluginResult, Transformer};
use serde_json::{Value, json};
use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

/// Prefix of the CSS classes on highlighted tokens.
pub const CLASS_PREFIX: &str = "hl-";

/// The syntaxes bundled with `two-face` (those of `syntect` plus `bat`'s
/// extras, such as TOML and TypeScript), loaded on first use.
static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

/// The plugin, as registered with the host.
pub fn plugin() -> Plugin {
    Plugin::new("transform-highlight", env!("CARGO_PKG_VERSION"))
        .transformer(Highlight)
        .cacheable()
}

/// The highlighting transformer.
#[derive(Debug, Clone, Copy, Default)]
pub struct Highlight;

impl Transformer for Highlight {
    fn nodes(&self) -> Vec<String> {
        vec!["code_block".into()]
    }

    async fn apply(
        &self,
        params: TransformApplyParams,
        ctx: &Context,
    ) -> PluginResult<TransformApplyResult> {
        let options = Options::from_value(ctx.options());
        let mut document = params.document;
        highlight_all(&mut document.children, &options);
        Ok(TransformApplyResult {
            document,
            diagnostics: vec![],
        })
    }
}

/// What to highlight.
#[derive(Debug, Default)]
struct Options {
    /// Only these languages, if given.
    languages: Option<Vec<String>>,
    /// Extra names for languages.
    aliases: BTreeMap<String, String>,
}

impl Options {
    fn from_value(value: &Value) -> Self {
        let languages = value.get("languages").and_then(Value::as_array).map(|l| {
            l.iter()
                .filter_map(Value::as_str)
                .map(str::to_lowercase)
                .collect()
        });
        let aliases = value
            .get("aliases")
            .and_then(Value::as_object)
            .map(|a| {
                a.iter()
                    .filter_map(|(k, v)| Some((k.to_lowercase(), v.as_str()?.to_owned())))
                    .collect()
            })
            .unwrap_or_default();
        Self { languages, aliases }
    }

    /// The syntax to highlight `language` with, if any.
    fn syntax(&self, language: &str) -> Option<&'static SyntaxReference> {
        let language = language.to_lowercase();
        let language = self.aliases.get(&language).map_or(language.as_str(), |a| a);
        if let Some(only) = &self.languages
            && !only.iter().any(|l| *l == language)
        {
            return None;
        }
        find_syntax(language)
    }
}

/// A bundled syntax by name (case-insensitive) or file extension.
fn find_syntax(language: &str) -> Option<&'static SyntaxReference> {
    let syntaxes = &*SYNTAXES;
    syntaxes.find_syntax_by_token(language).or_else(|| {
        syntaxes
            .syntaxes()
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(language))
    })
}

/// Highlights every code block in `nodes`, recursively.
fn highlight_all(nodes: &mut [Node], options: &Options) {
    for node in nodes {
        if let Node::CodeBlock(block) = node {
            let syntax = block.language.as_deref().and_then(|l| options.syntax(l));
            if let Some(syntax) = syntax
                && let Some(html) = highlight(block, syntax)
            {
                let block = std::mem::take(block);
                *node = Node::Custom(Custom {
                    span: block.span,
                    kind: "highlight".into(),
                    attrs: BTreeMap::from([
                        ("language".into(), json!(block.language)),
                        ("html".into(), json!(html)),
                    ]),
                    children: vec![Node::CodeBlock(block)],
                });
            }
        } else if let Some(children) = node.children_mut() {
            highlight_all(children, options);
        }
    }
}

/// The block as highlighted HTML, or `None` if `syntect` fails on it.
fn highlight(block: &CodeBlock, syntax: &SyntaxReference) -> Option<String> {
    let mut generator = ClassedHTMLGenerator::new_with_class_style(
        syntax,
        &SYNTAXES,
        ClassStyle::SpacedPrefixed {
            prefix: CLASS_PREFIX,
        },
    );
    for line in LinesWithEndings::from(&block.value) {
        generator
            .parse_html_for_line_which_includes_newline(line)
            .ok()?;
    }
    let language = block.language.as_deref().unwrap_or_default();
    Some(format!(
        "<pre class=\"highlight\"><code class=\"language-{}\">{}</code></pre>\n",
        escape_attr(language),
        generator.finalize()
    ))
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsdocs_plugin_sdk::doctree::{BlockQuote, Document};
    use lsdocs_plugin_sdk::protocol::InitializeParams;
    use lsdocs_plugin_sdk::testing;

    fn code(language: Option<&str>, value: &str) -> Node {
        Node::CodeBlock(CodeBlock {
            language: language.map(Into::into),
            value: value.into(),
            ..CodeBlock::default()
        })
    }

    async fn run(options: Value, children: Vec<Node>) -> Vec<Node> {
        let mut doc = Document::new("a.md");
        doc.children = children;
        let init = InitializeParams {
            options,
            ..InitializeParams::default()
        };
        let mut out = testing::run_transform(&plugin(), &init, vec![doc])
            .await
            .unwrap();
        out.remove(0).0.children
    }

    fn html(node: &Node) -> &str {
        let Node::Custom(custom) = node else {
            panic!("not highlighted: {node:?}")
        };
        assert_eq!(custom.kind, "highlight");
        assert!(matches!(custom.children[..], [Node::CodeBlock(_)]));
        custom.attrs["html"].as_str().unwrap()
    }

    #[tokio::test]
    async fn highlights_known_languages() {
        let out = run(
            Value::Null,
            vec![
                code(Some("rust"), "fn main() { let x = \"<hi>\"; }\n"),
                code(Some("RS"), "// comment\n"),
                code(None, "plain\n"),
                code(Some("no-such-language"), "x\n"),
                Node::BlockQuote(BlockQuote {
                    children: vec![code(Some("toml"), "a = 1\n")],
                    ..BlockQuote::default()
                }),
            ],
        )
        .await;
        let rust = html(&out[0]);
        assert!(
            rust.starts_with("<pre class=\"highlight\"><code class=\"language-rust\">"),
            "{rust}"
        );
        assert!(
            rust.contains("<span class=\"hl-storage hl-type hl-function hl-rust\">fn</span>"),
            "{rust}"
        );
        // Code text is escaped.
        assert!(rust.contains("&lt;hi&gt;"), "{rust}");
        assert!(html(&out[1]).contains("hl-comment"));
        assert!(matches!(out[2], Node::CodeBlock(_)));
        assert!(matches!(out[3], Node::CodeBlock(_)));
        assert!(html(&out[4].children()[0]).contains("language-toml"));
    }

    #[tokio::test]
    async fn languages_and_aliases_options() {
        let out = run(
            json!({"languages": ["toml", "json"], "aliases": {"jsonc": "json"}}),
            vec![
                code(Some("rust"), "fn x() {}\n"),
                code(Some("jsonc"), "{\"a\": 1}\n"),
            ],
        )
        .await;
        assert!(matches!(out[0], Node::CodeBlock(_)));
        assert!(html(&out[1]).contains("hl-json"));
    }
}
