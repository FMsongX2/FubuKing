// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
#[cfg(atlas_engine_bazel)]
pub use code_mode_proto::atlas_engine::code_mode::v1::*;

#[cfg(not(atlas_engine_bazel))]
tonic::include_proto!("atlas_engine.code_mode.v1");

pub const MAX_IDENTIFIER_BYTES: usize = 256;
