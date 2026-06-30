use crate::design::Level;
use crate::diagram::{render_diagrams, run_claude, Diagram};
use crate::glossary::Term;
use crate::util::{lang_clause, tail_chars};
use serde::{Deserialize, Serialize};

/// Everything the viewer needs for one source, produced in a SINGLE Claude call
/// so the (potentially large) source is read only once and the three views stay
/// mutually consistent.
#[derive(Serialize, Deserialize, Clone)]
pub struct Bundle {
    pub levels: Vec<Level>,
    pub diagrams: Vec<Diagram>,
    pub terms: Vec<Term>,
}

const MAX_TRANSCRIPT_CHARS: usize = 400_000;

const PROMPT: &str = r#"You are a software-design explainer. Read the document below (a Claude Code design/architecture conversation, or an external design doc) and produce ONE JSON object with three coordinated parts: layered design levels, diagrams, and a glossary. Everything must be STRICTLY grounded in the document — never invent, assume, or add outside knowledge.

PART 1 — "levels": exactly three, in this order:
  1. "High-level"     — the big picture: purpose, the problem solved, main components/actors.
  2. "Detailed"       — how components interact: data flow, key design decisions and rationale, interfaces, trade-offs.
  3. "Implementation" — concrete specifics actually discussed: files, functions, APIs, schemas, configs, commands, edge cases.
  Each level has a short `title` (specific to this document) and `content` in concise Markdown (headings, bullets, fenced code allowed). If the document lacks detail for a level (common for "Implementation"), say so briefly instead of fabricating.

PART 2 — "diagrams": 1 to 5 diagrams that best help a reader UNDERSTAND the architecture and key decisions. Focus on structure, data flow, components, decisions — not chit-chat.
  - Prefer `kind:"mermaid"` for component relationships, flows, sequences, state machines, ER diagrams. Keep mermaid syntactically valid; do not wrap node labels in problematic characters.
  - Use `kind:"mingrammer"` (the Python `diagrams` library) ONLY for genuine cloud/infra topology (services, queues, databases, load balancers, providers). For mingrammer the Python code MUST use `show=False`, `outformat="svg"`, and `filename="diagram"` (exactly, no path/extension), importing only from the `diagrams` package.
  - Each diagram has a short `title`, the `source`, a 1-2 sentence `explanation`, and a `level` set to EXACTLY one of "High-level", "Detailed", or "Implementation" — the level it best illustrates. Spread diagrams across levels where it makes sense (e.g. an overview at "High-level", interaction/sequence at "Detailed").

PART 3 — "terms": 5 to 25 key technical terms, acronyms, tools, services, libraries, or domain jargon appearing in the document, ordered by importance to understanding the design. Each `definition` (1-2 sentences) reflects how the term is actually used in THIS document's context, not a generic dictionary gloss. Optionally set a short `category` ("Service", "Protocol", "Library", "Concept", "Tool", "Acronym"). Skip trivial or universally-known programming words.

Output ONLY a single JSON object, with NO markdown code fences and NO prose before or after, exactly matching this shape:
{"levels":[{"level":"High-level","title":"string","content":"string"},{"level":"Detailed","title":"string","content":"string"},{"level":"Implementation","title":"string","content":"string"}],"diagrams":[{"title":"string","kind":"mermaid","source":"string","explanation":"string","level":"High-level"}],"terms":[{"term":"string","definition":"string","category":"string"}]}

Here is the document:

"#;

pub fn generate(transcript: &str, model: &str, lang: &str) -> Result<Bundle, String> {
    let prompt = format!(
        "{PROMPT}{}{}",
        tail_chars(transcript, MAX_TRANSCRIPT_CHARS),
        lang_clause(lang)
    );
    // One retry on parse failure (models occasionally add stray prose).
    let mut last_err = String::new();
    for attempt in 0..2 {
        let result = run_claude(&prompt, model)?;
        match parse(&result) {
            Ok(mut bundle) => {
                render_diagrams(&mut bundle.diagrams);
                return Ok(bundle);
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

fn parse(raw: &str) -> Result<Bundle, String> {
    let trimmed = raw.trim();
    let inner = match (trimmed.find('{'), trimmed.rfind('}')) {
        (Some(a), Some(b)) if b >= a => &trimmed[a..=b],
        _ => return Err("no JSON object found in model output".to_string()),
    };
    serde_json::from_str::<Bundle>(inner).map_err(|e| format!("could not parse bundle JSON: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_bundle() {
        let raw = r##"prefix {"levels":[{"level":"High-level","title":"O","content":"# x"}],
          "diagrams":[{"title":"T","kind":"mermaid","source":"graph TD; A-->B","explanation":"e","level":"High-level"}],
          "terms":[{"term":"SQS","definition":"queue","category":"Service"}]} suffix"##;
        let b = parse(raw).unwrap();
        assert_eq!(b.levels.len(), 1);
        assert_eq!(b.diagrams[0].level, "High-level");
        assert_eq!(b.terms[0].term, "SQS");
    }

    #[test]
    fn defaults_when_optional_fields_missing() {
        let raw = r#"{"levels":[],"diagrams":[{"title":"T","kind":"mermaid","source":"s","explanation":"e"}],"terms":[{"term":"X","definition":"y"}]}"#;
        let b = parse(raw).unwrap();
        assert_eq!(b.diagrams[0].level, "");
        assert_eq!(b.terms[0].category, "");
    }
}
