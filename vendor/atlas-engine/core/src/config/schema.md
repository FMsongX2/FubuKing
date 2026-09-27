<!-- Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md. -->
# Config JSON Schema

We generate a JSON Schema for `~/.atlas_engine/config.toml` from the `ConfigToml` type
and commit it at `atlas-engine-rs/core/config.schema.json` for editor integration.

When you change any fields included in `ConfigToml` (or nested config types),
regenerate the schema:

```
just write-config-schema
```
