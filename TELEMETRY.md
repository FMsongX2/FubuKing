<!-- Modified by FubuKing from upstream Atlas (Apache-2.0). -->
# Telemetry and network access

FubuKing sends no telemetry.

- Release builds carry no analytics key. Upstream Atlas's PostHog client is still in the code, and without a key it is constructed inert: nothing is queued, nothing is sent, and the frontend never loads `posthog-js`.
- The "Share usage data" setting defaults to off and is hidden, because it would be a switch that does nothing.
- The updater upstream Atlas drives through analytics feature flags is inert for the same reason. FubuKing releases are published on GitHub.
- A build made with an analytics key supplied through `ATLAS_POSTHOG_KEY` is not a FubuKing release build.

## Hosted services

FubuKing does not use Atlas's hosted services: sign-in, organisations, team chat, checkpoint sync, shared artifacts, the AI gateway and its credits. The backend refuses to start sign-in (`src-tauri/src/hosted.rs`), and every one of those services is reached only through a signed-in account. There is no FubuKing server.

## Network requests the app makes

Each of these is a public third-party endpoint, reached only for the feature named.

| Endpoint | When |
| --- | --- |
| `cdn.agentclientprotocol.com`, `raw.githubusercontent.com/agentclientprotocol` | Listing and installing ACP agents from the registry |
| `github.com`, `api.github.com` | Downloading agent releases, GitHub features you use, skill and pack installs |
| `nodejs.org` | Installing the managed Node runtime some agents need |
| `huggingface.co` | Downloading the on-device embedding model for memory search |
| `models.dev` | Model price list for the usage dashboard |
| `www.skills.sh`, `open-vsx.org` | Searching skills and editor extensions |
| arXiv, Semantic Scholar | The research tab, when you search |
| The model provider you configure | Agents, model chat and memory extraction with your own keys or logins |

Agents themselves (Claude Code, Codex and others) make their own network requests under their own policies.
