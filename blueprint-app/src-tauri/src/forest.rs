use crate::diagram::{render_diagrams, run_claude, Diagram};
use crate::glossary::Term;
use crate::util::{lang_clause, tail_chars};
use serde::{Deserialize, Serialize};

/// A selectable "tree" — a component / subsystem / topic worth drilling into.
#[derive(Serialize, Deserialize, Clone)]
pub struct TreeRef {
    /// Stable-ish slug used as the cache key for the tree's detail.
    pub id: String,
    pub name: String,
    /// One-line description shown on the picker card.
    #[serde(default)]
    pub blurb: String,
}

/// The "forest": the big-picture overview plus the trees the reader can explore.
#[derive(Serialize, Deserialize, Clone)]
pub struct Forest {
    /// Markdown overview of the whole design.
    pub overview: String,
    pub diagrams: Vec<Diagram>,
    pub terms: Vec<Term>,
    pub trees: Vec<TreeRef>,
}

/// One explored "tree": an in-depth look at a single component/topic.
#[derive(Serialize, Deserialize, Clone)]
pub struct Tree {
    pub name: String,
    /// Markdown.
    pub content: String,
    pub diagrams: Vec<Diagram>,
    pub terms: Vec<Term>,
}

const MAX_TRANSCRIPT_CHARS: usize = 160_000;

/// Mermaid rules shared across prompts to cut down on syntax that fails to parse.
const MERMAID_RULES: &str = "For any `mermaid` diagram, keep it small and STRICTLY valid: start with one of `flowchart TD`, `sequenceDiagram`, `erDiagram`, or `stateDiagram-v2`; use simple alphanumeric node ids (no spaces); put EVERY node and edge label in double quotes; never use parentheses, semicolons, colons, '#', '<', '>', '@', backticks, or line breaks inside a label; one statement per line.";

const DIAGRAM_RULES: &str = "Prefer `kind:\"mermaid\"`. Use `kind:\"mingrammer\"` (Python `diagrams` library, `show=False`, `outformat=\"svg\"`, `filename=\"diagram\"`, import only from `diagrams`) ONLY for real cloud/infra topology.";

fn forest_prompt(transcript: &str, lang: &str) -> String {
    format!(
        r#"You give a BIG-PICTURE overview of the design in the document below, then list the parts worth exploring in depth — STRICTLY grounded in the document. Produce:
- `overview`: concise Markdown explaining the whole thing at a glance — purpose, the problem solved, the main components/actors and how they fit together. This is the "forest", not the details.
- `diagrams`: 0 to 2 diagrams for the overall architecture/flow. {diagram} {mermaid}
- `terms`: 3 to 10 key terms appearing in the overview, each with a 1-2 sentence definition reflecting how it is used in this document.
- `trees`: 3 to 8 components/subsystems/topics that a reader could drill into for more detail. Each has an `id` (short lowercase slug, e.g. "auth-flow"), a `name` (human label), and a one-line `blurb`. Pick the parts that genuinely have depth in the document.

CRITICAL: Use ONLY information present in the document. Do NOT invent or add outside knowledge.

Output ONLY a single JSON object, NO markdown code fences and NO prose before/after, exactly matching:
{{"overview":"string","diagrams":[{{"title":"string","kind":"mermaid","source":"string","explanation":"string"}}],"terms":[{{"term":"string","definition":"string","category":"string"}}],"trees":[{{"id":"string","name":"string","blurb":"string"}}]}}

Here is the document:

"#,
        diagram = DIAGRAM_RULES,
        mermaid = MERMAID_RULES,
    ) + &format!("{}{}", tail_chars(transcript, MAX_TRANSCRIPT_CHARS), lang_clause(lang))
}

fn tree_prompt(transcript: &str, tree_name: &str, lang: &str) -> String {
    format!(
        r#"From the document below, explain IN DEPTH this one part: "{name}". Focus only on this part; STRICTLY grounded in the document. Produce:
- `content`: detailed Markdown about "{name}" — how it works, its responsibilities, data flow, key decisions, interfaces, and any concrete implementation specifics actually discussed (files, functions, APIs, configs, edge cases). If the document lacks detail, say so briefly instead of fabricating.
- `diagrams`: 0 to 2 diagrams that illustrate THIS part. {diagram} {mermaid}
- `terms`: 3 to 10 key terms appearing in this explanation, each with a 1-2 sentence definition in this document's context.

CRITICAL: Use ONLY information present in the document. Do NOT invent or add outside knowledge.

Output ONLY a single JSON object, NO markdown code fences and NO prose before/after, exactly matching:
{{"name":"{name}","content":"string","diagrams":[{{"title":"string","kind":"mermaid","source":"string","explanation":"string"}}],"terms":[{{"term":"string","definition":"string","category":"string"}}]}}

Here is the document:

"#,
        name = tree_name,
        diagram = DIAGRAM_RULES,
        mermaid = MERMAID_RULES,
    ) + &format!("{}{}", tail_chars(transcript, MAX_TRANSCRIPT_CHARS), lang_clause(lang))
}

pub fn generate_forest(transcript: &str, model: &str, lang: &str) -> Result<Forest, String> {
    let prompt = forest_prompt(transcript, lang);
    let mut last_err = String::new();
    for attempt in 0..2 {
        let result = run_claude(&prompt, model)?;
        match parse_forest(&result) {
            Ok(mut f) => {
                render_diagrams(&mut f.diagrams);
                return Ok(f);
            }
            Err(e) => {
                last_err = e;
                if attempt == 0 {
                    continue;
                }
            }
        }
    }
    Err(last_err)
}

pub fn generate_tree(
    transcript: &str,
    tree_name: &str,
    model: &str,
    lang: &str,
) -> Result<Tree, String> {
    let prompt = tree_prompt(transcript, tree_name, lang);
    let mut last_err = String::new();
    for attempt in 0..2 {
        let result = run_claude(&prompt, model)?;
        match parse_tree(&result) {
            Ok(mut t) => {
                if t.name.is_empty() {
                    t.name = tree_name.to_string();
                }
                render_diagrams(&mut t.diagrams);
                return Ok(t);
            }
            Err(e) => {
                last_err = e;
                if attempt == 0 {
                    continue;
                }
            }
        }
    }
    Err(last_err)
}

fn json_slice(raw: &str) -> Result<&str, String> {
    let trimmed = raw.trim();
    match (trimmed.find('{'), trimmed.rfind('}')) {
        (Some(a), Some(b)) if b >= a => Ok(&trimmed[a..=b]),
        _ => Err("no JSON object found in model output".to_string()),
    }
}

fn parse_forest(raw: &str) -> Result<Forest, String> {
    serde_json::from_str::<Forest>(json_slice(raw)?)
        .map_err(|e| format!("could not parse forest JSON: {e}"))
}

fn parse_tree(raw: &str) -> Result<Tree, String> {
    serde_json::from_str::<Tree>(json_slice(raw)?)
        .map_err(|e| format!("could not parse tree JSON: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_forest() {
        let raw = r##"x {"overview":"# o","diagrams":[],"terms":[{"term":"T","definition":"d","category":""}],
          "trees":[{"id":"auth","name":"Auth","blurb":"login"}]} y"##;
        let f = parse_forest(raw).unwrap();
        assert_eq!(f.trees.len(), 1);
        assert_eq!(f.trees[0].id, "auth");
    }

    #[test]
    fn parses_tree() {
        let raw = r#"{"name":"Auth","content":"detail","diagrams":[{"title":"T","kind":"mermaid","source":"flowchart TD","explanation":"e"}],"terms":[]}"#;
        let t = parse_tree(raw).unwrap();
        assert_eq!(t.name, "Auth");
        assert_eq!(t.diagrams.len(), 1);
    }
}
