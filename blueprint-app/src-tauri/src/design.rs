use crate::diagram::run_claude;
use crate::util::{lang_clause, tail_chars};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct Level {
    /// "High-level" | "Detailed" | "Implementation"
    pub level: String,
    pub title: String,
    /// Markdown.
    pub content: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DesignDoc {
    pub levels: Vec<Level>,
}

const MAX_TRANSCRIPT_CHARS: usize = 400_000;

const PROMPT: &str = r#"You explain a software design at progressive levels of detail, STRICTLY grounded in the document provided below. Produce exactly three levels, in this order:
1. "High-level" — the big picture: purpose, the problem being solved, and the main components/actors.
2. "Detailed" — how the components interact: data flow, key design decisions and their rationale, interfaces, trade-offs.
3. "Implementation" — concrete implementation specifics actually discussed: specific files, functions, APIs, schemas, configs, commands, and edge cases.

CRITICAL RULES:
- Use ONLY information that is actually present in the document. Do NOT invent, assume, or add outside knowledge.
- If the document does not contain enough detail for a level (this often happens for "Implementation"), say so briefly (e.g. "The document does not specify implementation details for X") instead of fabricating.
- Each level's `content` is concise Markdown (headings, bullet lists, and fenced code are allowed).
- Each level needs a short `title` summarizing that level for this specific document.

Output ONLY a single JSON object, with NO markdown code fences and NO prose before or after, exactly matching:
{"levels":[{"level":"High-level","title":"string","content":"string"},{"level":"Detailed","title":"string","content":"string"},{"level":"Implementation","title":"string","content":"string"}]}

Here is the document:

"#;

pub fn generate(transcript: &str, model: &str, lang: &str) -> Result<DesignDoc, String> {
    let prompt = format!(
        "{PROMPT}{}{}",
        tail_chars(transcript, MAX_TRANSCRIPT_CHARS),
        lang_clause(lang)
    );
    let mut last_err = String::new();
    for attempt in 0..2 {
        let result = run_claude(&prompt, model)?;
        match parse(&result) {
            Ok(doc) => return Ok(doc),
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

fn parse(raw: &str) -> Result<DesignDoc, String> {
    let trimmed = raw.trim();
    let inner = match (trimmed.find('{'), trimmed.rfind('}')) {
        (Some(a), Some(b)) if b >= a => &trimmed[a..=b],
        _ => return Err("no JSON object found in model output".to_string()),
    };
    serde_json::from_str::<DesignDoc>(inner)
        .map_err(|e| format!("could not parse design JSON: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_levels() {
        let raw = r##"{"levels":[
          {"level":"High-level","title":"Overview","content":"# x"},
          {"level":"Detailed","title":"Flow","content":"- a"},
          {"level":"Implementation","title":"Code","content":"`f()`"}]}"##;
        let doc = parse(raw).unwrap();
        assert_eq!(doc.levels.len(), 3);
        assert_eq!(doc.levels[0].level, "High-level");
    }
}
