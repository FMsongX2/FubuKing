#!/usr/bin/env bash
# Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
minimum_supported_release="$(
  sed -n 's/^pub const MINIMUM_SUPPORTED_ATLAS_AGENT_VERSION: &str = "\([^"]*\)";$/\1/p' \
    "${repo_root}/atlas-engine-rs/exec-server-protocol/src/lib.rs"
)"
: "${minimum_supported_release:?minimum supported Atlas Agent release is missing}"
release_directory="$(mktemp -d "${TMPDIR:-/tmp}/atlas-engine-exec-server-skew.XXXXXX")"
trap 'rm -rf "${release_directory:?}"' EXIT

if [[ $# -eq 0 ]]; then
  releases=(latest "${minimum_supported_release}")
else
  releases=("$@")
fi

case "$(uname -s):$(uname -m)" in
  Darwin:arm64) target="aarch64-apple-darwin" ;;
  Darwin:x86_64) target="x86_64-apple-darwin" ;;
  Linux:aarch64 | Linux:arm64) target="aarch64-unknown-linux-musl" ;;
  Linux:x86_64) target="x86_64-unknown-linux-musl" ;;
  *)
    echo "Unsupported platform: $(uname -s) $(uname -m)" >&2
    exit 1
    ;;
esac

asset="atlas-engine-${target}.tar.gz"
cd "${repo_root}/atlas-engine-rs"
cargo build -p atlas-engine-cli --bin atlas-agent
export ATLAS_AGENT_TEST_CURRENT_ATLAS_ENGINE="${CARGO_TARGET_DIR:-${repo_root}/atlas-engine-rs/target}/debug/atlas-agent"

echo "Testing current Atlas Agent compatibility through authenticated Noise"
export ATLAS_AGENT_TEST_RELEASED_ATLAS_ENGINE="${ATLAS_AGENT_TEST_CURRENT_ATLAS_ENGINE}"
just test -p atlas-engine-exec-server --test relay version_skew --test-threads 1

tested_release_version=""
for release in "${releases[@]}"; do
  release="${release#rust-v}"
  if [[ "${release}" == "${tested_release_version}" ]]; then
    echo "Skipping Atlas Agent ${release}; this release was already tested"
    continue
  fi

  if [[ "${release}" == "latest" ]]; then
    release_url="https://github.com/openai/codex/releases/latest/download/${asset}"
  else
    release_url="https://github.com/openai/codex/releases/download/rust-v${release}/${asset}"
  fi

  binary_directory="${release_directory}/${release}"
  mkdir -p "${binary_directory}"
  echo "Downloading released Atlas Agent from ${release_url}"
  curl -fsSL "${release_url}" -o "${binary_directory}/${asset}"
  tar -xzf "${binary_directory}/${asset}" -C "${binary_directory}"

  export ATLAS_AGENT_TEST_RELEASED_ATLAS_ENGINE="${binary_directory}/atlas-engine-${target}"
  release_output="$("${ATLAS_AGENT_TEST_RELEASED_ATLAS_ENGINE}" --version)"
  echo "${release_output}"
  tested_release_version="${release_output##* }"

  just test -p atlas-engine-exec-server --test relay version_skew --test-threads 1
done
