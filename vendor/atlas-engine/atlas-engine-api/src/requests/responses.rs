// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Compression {
    #[default]
    None,
    Zstd,
}
