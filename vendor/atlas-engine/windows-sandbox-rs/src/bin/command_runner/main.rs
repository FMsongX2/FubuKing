// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
#[cfg(target_os = "windows")]
mod win;

#[cfg(target_os = "windows")]
fn main() -> anyhow::Result<()> {
    win::main()
}

#[cfg(not(target_os = "windows"))]
fn main() {
    panic!("atlas-engine-command-runner is Windows-only");
}
