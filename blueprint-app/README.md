# Monet

Turn Claude Code design/architecture sessions — and external design docs — into
diagrams, a layered design walkthrough, and a contextual glossary.

Pick a source and Monet asks your **local** Claude Code CLI (`claude -p`,
headless) to read it **once** and produce, in a single pass:

- **Diagrams** — `mermaid` for flows/sequences/relationships, plus
  `mingrammer/diagrams` for cloud/infra topology when appropriate.
- **Design** — the document split into **High-level → Detailed →
  Implementation**, viewed one level at a time, strictly grounded in what the
  document actually says (it won't invent specifics).
- **Terms** — a glossary of the technical terms, defined in this document's
  context.

No API keys: it reuses your existing Claude Code authentication.

## How it works

```
Tauri app
  webview (UI)  <-- IPC -->  Rust backend
   · source list              · scan ~/.claude/projects, parse JSONL
   · mermaid rendering         · resolve source text (session JSONL, or
   · infra SVG display           link sources fetched via gh / Notion connector)
   · one-at-a-time design      · claude -p  → ONE JSON bundle
   · transcript / source       · python3 + graphviz (mingrammer render)
                               · disk cache (keyed by source path + mtime + lang)
```

A **single** `claude -p` call returns one JSON bundle (`levels` + `diagrams` +
`terms`), so a large source is read only once and the three tabs stay mutually
consistent. Each diagram is tagged with the design level it illustrates, and the
Design tab shows those diagrams inline beneath the matching level.

## Requirements

- **Claude Code CLI** (`claude`) on PATH — required.
- **Graphviz + Python `diagrams`** — optional, only for mingrammer infra
  diagrams. Without them, mermaid still works; infra diagrams show a hint.
  - `brew install graphviz && pip install diagrams`

## Develop

```bash
npm install
npm run tauri dev
```

## Build a .app

```bash
npm run tauri build
# bundle: src-tauri/target/release/bundle/macos/
```

## Sources

- **Claude Code sessions** — scanned from `~/.claude/projects/*.jsonl`.
- **Links** — "Import from link" with one or more GitHub and/or Notion URLs
  (they can be mixed). Monet asks your local Claude Code to fetch them at
  generation time — GitHub via the `gh` CLI, Notion via the Claude Notion MCP
  connector. **No API keys.** The fetched content is viewable in the **Source**
  tab and cached as a local `.fetched.md`.

## Tabs

- **Diagrams** — gallery of mermaid (+ optional mingrammer infra) diagrams, each
  with a level badge; expand any diagram to a lightbox.
- **Design** — High-level → Detailed → Implementation, one level at a time, with
  that level's diagrams inline.
- **Terms** — contextual technical glossary.
- **Transcript / Source** — the extracted session transcript, or (for link
  sources) the fetched source content.

One **Generate** button (in the header and in each empty tab) produces all three
at once; once a bundle is cached it becomes **Regenerate**. Results are cached by
source path + mtime, namespaced per language.

## Settings

- **Language** (English / 한국어) — language of generated natural-language text.
- **Model** (header): Fast = Haiku, Balanced = Sonnet, Best = Opus.

## Layout

- `src-tauri/src/session.rs` — scan + JSONL parse + transcript extraction
- `src-tauri/src/imported.rs` — link sources: `.links.json` manifests, dynamic
  fetch (gh / Notion connector) → cached `.fetched.md`
- `src-tauri/src/bundle.rs` — the single combined generation (levels + diagrams
  + terms): prompt, parse, mingrammer render
- `src-tauri/src/diagram.rs` — `claude` invocation, mingrammer render, dep check
- `src-tauri/src/design.rs` / `glossary.rs` — the `Level` / `Term` types
- `src-tauri/src/cache.rs` — artifact cache (namespace + path + mtime key)
- `src-tauri/src/util.rs` — binary discovery, isolated work dir, language clause
- `src/main.ts` — UI

Generation uses a stripped headless `claude` (`--strict-mcp-config`,
`--setting-sources ""`, strict-JSON system prompt) for speed and reliable
parsing. Default model is `sonnet` (`DEFAULT_MODEL` in `lib.rs`). Spawned
`claude` runs in an isolated work dir to avoid macOS privacy prompts.

## Releases & in-app updates

The app self-updates via the Tauri updater, reading `latest.json` from the
GitHub Releases of `jngyunkim/blueprint`.

To cut a release:

```bash
# bump version in src-tauri/tauri.conf.json + package.json, then:
git tag v0.1.13 && git push origin v0.1.13
```

The `.github/workflows/release.yml` workflow builds on a macOS runner, signs the
update with the private key (repo secrets `TAURI_SIGNING_PRIVATE_KEY` /
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`), and publishes the `.dmg`, the updater
tarball + `.sig`, and `latest.json`. Running apps then see the update on launch
or via "Check for updates".

> The signing key lives at `~/.tauri/blueprint_updater.key` (keep it safe — it
> is **not** in the repo). The matching public key is in `tauri.conf.json`.
