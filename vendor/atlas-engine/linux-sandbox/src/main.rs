// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
/// Note that the cwd, env, and command args are preserved in the ultimate call
/// to `execv`, so the caller is responsible for ensuring those values are
/// correct.
fn main() -> ! {
    atlas_engine_linux_sandbox::run_main()
}
