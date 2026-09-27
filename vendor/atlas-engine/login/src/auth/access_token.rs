// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
const PERSONAL_ACCESS_TOKEN_PREFIX: &str = "at-";

pub(super) enum AtlasEngineAccessToken<'a> {
    PersonalAccessToken(&'a str),
    AgentIdentityJwt(&'a str),
}

pub(super) fn classify_atlas_engine_access_token(access_token: &str) -> AtlasEngineAccessToken<'_> {
    if access_token.starts_with(PERSONAL_ACCESS_TOKEN_PREFIX) {
        AtlasEngineAccessToken::PersonalAccessToken(access_token)
    } else {
        AtlasEngineAccessToken::AgentIdentityJwt(access_token)
    }
}

#[cfg(test)]
#[path = "access_token_tests.rs"]
mod tests;
