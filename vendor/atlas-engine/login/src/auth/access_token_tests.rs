// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use super::*;

#[test]
fn classifies_personal_access_tokens_by_prefix() {
    assert!(matches!(
        classify_atlas_engine_access_token("at-example"),
        AtlasEngineAccessToken::PersonalAccessToken("at-example")
    ));
    assert!(matches!(
        classify_atlas_engine_access_token("header.payload.signature"),
        AtlasEngineAccessToken::AgentIdentityJwt("header.payload.signature")
    ));
}
