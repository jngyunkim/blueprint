mod bundle;
mod cache;
mod design;
mod diagram;
mod glossary;
mod imported;
mod session;
mod util;

use bundle::Bundle;
use diagram::DepStatus;
use session::SessionMeta;

const DEFAULT_MODEL: &str = "sonnet";

fn lang_of(lang: Option<String>) -> String {
    match lang.as_deref() {
        Some("ko") => "ko".to_string(),
        _ => "en".to_string(),
    }
}

/// Resolve any source to its text: link sources fetch their URLs dynamically;
/// Notion/web `.md` are raw; Claude Code `.jsonl` are transcript-extracted.
fn source_transcript(path: &str) -> Result<String, String> {
    if imported::is_link_source(path) {
        imported::resolve(path)
    } else {
        session::resolve_transcript(path)
    }
}

/// Only allow the three speed/quality tiers the UI exposes; fall back to the
/// default for anything unexpected.
fn resolve_model(model: Option<String>) -> String {
    match model.as_deref() {
        Some("haiku") | Some("sonnet") | Some("opus") => model.unwrap(),
        _ => DEFAULT_MODEL.to_string(),
    }
}

#[tauri::command]
async fn list_sessions() -> Vec<SessionMeta> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut all = session::list_sessions();
        all.extend(imported::list_sources());
        all.sort_by(|a, b| b.modified.cmp(&a.modified));
        all
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
async fn get_transcript(path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || source_transcript(&path))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn check_deps() -> DepStatus {
    tauri::async_runtime::spawn_blocking(diagram::check_deps)
        .await
        .unwrap_or(DepStatus {
            claude: false,
            python: false,
            graphviz: false,
            diagrams_pkg: false,
        })
}

/// Return the cached bundle (design levels + diagrams + terms) for a source
/// without invoking Claude. Returns null when nothing is cached, so the UI can
/// show an explicit "Generate" button.
#[tauri::command]
async fn cached_bundle(path: String, lang: Option<String>) -> Option<Bundle> {
    let lang = lang_of(lang);
    tauri::async_runtime::spawn_blocking(move || {
        let mtime = cache::mtime_of(&path);
        cache::load(&format!("bundle-{lang}"), &path, mtime)
    })
    .await
    .ok()
    .flatten()
}

/// Generate the full bundle in a single Claude call (one read of the source),
/// caching the result. `force` bypasses the cache to regenerate.
#[tauri::command]
async fn generate_bundle(
    path: String,
    force: bool,
    model: Option<String>,
    lang: Option<String>,
) -> Result<Bundle, String> {
    let model = resolve_model(model);
    let lang = lang_of(lang);
    tauri::async_runtime::spawn_blocking(move || {
        let mtime = cache::mtime_of(&path);
        let ns = format!("bundle-{lang}");
        if !force {
            if let Some(cached) = cache::load(&ns, &path, mtime) {
                return Ok(cached);
            }
        }
        let transcript = source_transcript(&path)?;
        if transcript.trim().is_empty() {
            return Err("This source has no readable text.".to_string());
        }
        let bundle = bundle::generate(&transcript, &model, &lang)?;
        cache::save(&ns, &path, mtime, &bundle);
        Ok(bundle)
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---------- Imported link sources ----------

/// Create a link source from one or more GitHub / Notion / web URLs. Content is
/// fetched dynamically (via the local Claude CLI) at generation time — no keys.
#[tauri::command]
async fn import_link(urls: Vec<String>, title: Option<String>) -> Result<SessionMeta, String> {
    tauri::async_runtime::spawn_blocking(move || imported::create_source(urls, title))
        .await
        .map_err(|e| e.to_string())?
}

/// Force a link source to re-fetch its URLs on the next generation.
#[tauri::command]
async fn refresh_source(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || imported::refresh(&path))
        .await
        .map_err(|e| e.to_string())?
}

/// Resolve `path` to a canonical path, rejecting anything that does not live
/// under `base`. Both sides are canonicalized: on macOS canonicalize() resolves
/// the /Users firmlink to /System/Volumes/Data/Users, so comparing against a
/// non-canonical base would always fail.
fn resolve_within(path: &str, base: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let base = base
        .canonicalize()
        .map_err(|e| format!("could not resolve projects dir: {e}"))?;
    let canonical = std::path::Path::new(path)
        .canonicalize()
        .map_err(|e| format!("session not found: {e}"))?;
    if !canonical.starts_with(&base) {
        return Err("refusing to delete a file outside ~/.claude/projects".to_string());
    }
    Ok(canonical)
}

/// Move a source to the OS trash (recoverable). Allows Claude Code sessions
/// (~/.claude/projects) and imported Notion sources (the app data dir).
#[tauri::command]
async fn delete_session(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let projects = dirs::home_dir()
            .map(|h| h.join(".claude").join("projects"))
            .ok_or("could not resolve home directory")?;
        let canonical = resolve_within(&path, &projects)
            .or_else(|_| resolve_within(&path, &imported::sources_dir()))
            .map_err(|_| {
                "refusing to delete a file outside the session or Notion source folders"
                    .to_string()
            })?;
        trash::delete(&canonical).map_err(|e| format!("could not move to Trash: {e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::resolve_within;
    use std::fs;

    #[test]
    fn accepts_file_inside_base_and_rejects_outside() {
        let root = std::env::temp_dir().join(format!("bp-del-{}", std::process::id()));
        let base = root.join("projects");
        let proj = base.join("proj");
        fs::create_dir_all(&proj).unwrap();
        let inside = proj.join("s.jsonl");
        fs::write(&inside, "{}").unwrap();
        let outside = root.join("evil.jsonl");
        fs::write(&outside, "{}").unwrap();

        assert!(resolve_within(inside.to_str().unwrap(), &base).is_ok());
        assert!(resolve_within(outside.to_str().unwrap(), &base).is_err());

        let _ = fs::remove_dir_all(&root);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            list_sessions,
            get_transcript,
            check_deps,
            cached_bundle,
            generate_bundle,
            import_link,
            refresh_source,
            delete_session
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
