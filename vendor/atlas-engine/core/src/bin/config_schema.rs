// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

/// Generate the JSON Schema for `config.toml` and write it to `config.schema.json`.
#[derive(Parser)]
#[command(name = "atlas-engine-write-config-schema")]
struct Args {
    #[arg(short, long, value_name = "PATH")]
    out: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let out_path = args
        .out
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config.schema.json"));
    atlas_engine_config::schema::write_config_schema(&out_path)?;
    Ok(())
}
