// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use super::*;

#[test]
fn device_code_prompt_renders_phishing_warning() {
    let prompt = device_code_prompt("https://example.com/device", "ABCD-EFGH");

    assert!(prompt.contains(
        "\x1b[90mContinue only if you started this login in Atlas Agent. If a website or another person gave you this code, cancel.\x1b[0m"
    ));
}
