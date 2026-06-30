use crate::session::SessionMeta;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

const NOTION_VERSION: &str = "2022-06-28";

pub fn sources_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("blueprint")
        .join("notion-sources")
}

/// Extract a Notion page id (formatted as a dashed UUID) from a page URL.
/// The id is the trailing 32 hex digits of the last URL path segment.
pub fn parse_page_id(url: &str) -> Option<String> {
    let no_query = url.split(['?', '#']).next().unwrap_or(url);
    let seg = no_query.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let hex: String = seg.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() < 32 {
        return None;
    }
    let id = &hex[hex.len() - 32..];
    Some(format!(
        "{}-{}-{}-{}-{}",
        &id[0..8],
        &id[8..12],
        &id[12..16],
        &id[16..20],
        &id[20..32]
    ))
}

fn api_get(path: &str, token: &str) -> Result<Value, String> {
    let resp = ureq::get(&format!("https://api.notion.com/v1/{path}"))
        .set("Authorization", &format!("Bearer {token}"))
        .set("Notion-Version", NOTION_VERSION)
        .call();
    match resp {
        Ok(r) => r.into_json().map_err(|e| e.to_string()),
        Err(ureq::Error::Status(code, r)) => {
            let body = r.into_string().unwrap_or_default();
            let hint = if code == 401 {
                " (check your Notion token in Settings)"
            } else if code == 404 {
                " (page not found, or the integration has not been shared with this page)"
            } else {
                ""
            };
            Err(format!(
                "Notion API error {code}{hint}: {}",
                body.chars().take(160).collect::<String>()
            ))
        }
        Err(e) => Err(format!("could not reach Notion: {e}")),
    }
}

fn rich_text(arr: &[Value]) -> String {
    arr.iter()
        .filter_map(|e| e.get("plain_text").and_then(|p| p.as_str()))
        .collect::<Vec<_>>()
        .join("")
}

fn fetch_title(id: &str, token: &str) -> Result<String, String> {
    let page = api_get(&format!("pages/{id}"), token)?;
    if let Some(props) = page.get("properties").and_then(|p| p.as_object()) {
        for v in props.values() {
            if v.get("type").and_then(|t| t.as_str()) == Some("title") {
                if let Some(arr) = v.get("title").and_then(|t| t.as_array()) {
                    let t = rich_text(arr);
                    if !t.trim().is_empty() {
                        return Ok(t);
                    }
                }
            }
        }
    }
    Ok("Notion page".to_string())
}

fn block_to_text(b: &Value, depth: usize, out: &mut String) {
    let t = b.get("type").and_then(|x| x.as_str()).unwrap_or("");
    let inner = b.get(t);
    let text = inner
        .and_then(|i| i.get("rich_text"))
        .and_then(|r| r.as_array())
        .map(|a| rich_text(a))
        .unwrap_or_default();
    let indent = "  ".repeat(depth.saturating_sub(1));
    match t {
        "heading_1" => out.push_str(&format!("\n# {text}\n")),
        "heading_2" => out.push_str(&format!("\n## {text}\n")),
        "heading_3" => out.push_str(&format!("\n### {text}\n")),
        "bulleted_list_item" | "toggle" => out.push_str(&format!("{indent}- {text}\n")),
        "numbered_list_item" => out.push_str(&format!("{indent}1. {text}\n")),
        "to_do" => {
            let checked = inner
                .and_then(|i| i.get("checked"))
                .and_then(|c| c.as_bool())
                .unwrap_or(false);
            out.push_str(&format!("{indent}- [{}] {text}\n", if checked { "x" } else { " " }));
        }
        "quote" | "callout" => out.push_str(&format!("> {text}\n")),
        "code" => {
            let lang = inner
                .and_then(|i| i.get("language"))
                .and_then(|l| l.as_str())
                .unwrap_or("");
            out.push_str(&format!("```{lang}\n{text}\n```\n"));
        }
        "child_page" => {
            let title = inner
                .and_then(|i| i.get("title"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            out.push_str(&format!("\n## {title}\n"));
        }
        "divider" => out.push_str("\n---\n"),
        "paragraph" => {
            if !text.is_empty() {
                out.push_str(&format!("{text}\n"));
            }
        }
        _ => {
            if !text.is_empty() {
                out.push_str(&format!("{text}\n"));
            }
        }
    }
}

fn fetch_blocks(id: &str, token: &str, depth: usize, out: &mut String) -> Result<(), String> {
    if depth > 3 {
        return Ok(());
    }
    let mut cursor: Option<String> = None;
    loop {
        let mut path = format!("blocks/{id}/children?page_size=100");
        if let Some(c) = &cursor {
            path.push_str(&format!("&start_cursor={c}"));
        }
        let data = api_get(&path, token)?;
        if let Some(results) = data.get("results").and_then(|r| r.as_array()) {
            for b in results {
                block_to_text(b, depth, out);
                if b.get("has_children").and_then(|h| h.as_bool()).unwrap_or(false) {
                    if let Some(bid) = b.get("id").and_then(|i| i.as_str()) {
                        fetch_blocks(bid, token, depth + 1, out)?;
                    }
                }
            }
        }
        let has_more = data.get("has_more").and_then(|h| h.as_bool()).unwrap_or(false);
        match data.get("next_cursor").and_then(|c| c.as_str()) {
            Some(c) if has_more => cursor = Some(c.to_string()),
            _ => break,
        }
    }
    Ok(())
}

/// Fetch a Notion page, save it as a local markdown source, and return its meta.
pub fn import(url: &str, token: &str) -> Result<SessionMeta, String> {
    if token.trim().is_empty() {
        return Err("No Notion token set. Add one in Settings.".to_string());
    }
    let id = parse_page_id(url).ok_or("Could not find a Notion page id in that URL.")?;
    let title = fetch_title(&id, token)?;
    let mut body = String::new();
    fetch_blocks(&id, token, 0, &mut body)?;

    let dir = sources_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join(format!("{id}.md"));
    let content = format!("# {title}\n\n<!-- Source: {url} -->\n\n{body}");
    fs::write(&file, content).map_err(|e| e.to_string())?;

    Ok(SessionMeta {
        id: format!("notion:{id}"),
        path: file.to_string_lossy().to_string(),
        project: "Notion".to_string(),
        title,
        modified: now_secs(),
        message_count: 0,
    })
}

/// List previously imported Notion sources for the sidebar.
pub fn list_sources() -> Vec<SessionMeta> {
    let dir = sources_dir();
    let mut out = Vec::new();
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("md") {
            continue;
        }
        let content = fs::read_to_string(&p).unwrap_or_default();
        let title = content
            .lines()
            .find(|l| l.starts_with("# "))
            .map(|l| l[2..].trim().to_string())
            .unwrap_or_else(|| "Notion page".to_string());
        let modified = fs::metadata(&p)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let id = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        out.push(SessionMeta {
            id: format!("notion:{id}"),
            path: p.to_string_lossy().to_string(),
            project: "Notion".to_string(),
            title,
            modified,
            message_count: 0,
        });
    }
    out
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::parse_page_id;

    #[test]
    fn parses_id_from_titled_url() {
        let id =
            parse_page_id("https://www.notion.so/moloco/Docs-31ccdb35133680f89676fa9cdcd3dfce")
                .unwrap();
        assert_eq!(id, "31ccdb35-1336-80f8-9676-fa9cdcd3dfce");
    }

    #[test]
    fn parses_id_from_dashed_url_with_query() {
        let id = parse_page_id(
            "https://notion.so/31ccdb35-1336-80f8-9676-fa9cdcd3dfce?pvs=4",
        )
        .unwrap();
        assert_eq!(id, "31ccdb35-1336-80f8-9676-fa9cdcd3dfce");
    }

    #[test]
    fn rejects_non_notion() {
        assert!(parse_page_id("https://example.com/page").is_none());
    }
}
