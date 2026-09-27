<!-- Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md. -->
# atlas-engine-protocol

This crate defines the "types" for the protocol used by Atlas Agent CLI, which includes both "internal types" for communication between `atlas-engine-core` and `atlas-engine-tui`, as well as "external types" used with `atlas_engine app-server`.

This crate should have minimal dependencies.

Ideally, we should avoid "material business logic" in this crate, as we can always introduce `Ext`-style traits to add functionality to types in other crates.
