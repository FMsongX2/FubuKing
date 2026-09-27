// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let schema_root = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schema"));
    atlas_engine_hooks::write_schema_fixtures(&schema_root)
}
