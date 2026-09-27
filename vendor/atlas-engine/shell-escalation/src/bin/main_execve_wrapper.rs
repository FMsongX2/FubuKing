// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
#[cfg(not(unix))]
fn main() {
    eprintln!("atlas-engine-execve-wrapper is only implemented for UNIX");
    std::process::exit(1);
}

#[cfg(unix)]
pub use atlas_engine_shell_escalation::main_execve_wrapper as main;
