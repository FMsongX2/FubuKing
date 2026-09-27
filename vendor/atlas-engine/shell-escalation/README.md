<!-- Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md. -->
# atlas-engine-shell-escalation

This crate contains the Unix shell-escalation protocol implementation and the
`atlas-engine-execve-wrapper` executable.

`atlas-engine-execve-wrapper` receives the arguments to an intercepted `execve(2)` call and delegates the
decision to the shell-escalation protocol over a shared file descriptor (specified by the
`ATLAS_AGENT_ESCALATE_SOCKET` environment variable). The server on the other side replies with one of:

- `Run`: `atlas-engine-execve-wrapper` should invoke `execve(2)` on itself to run the original command
  within the sandboxed shell.
- `Escalate`: forward the file descriptors of the current process so the command can be run
  faithfully outside the sandbox. When the process completes, the server forwards the exit code
  back to `atlas-engine-execve-wrapper`.
- `Deny`: the server has declared the proposed command to be forbidden, so
  `atlas-engine-execve-wrapper` prints an error to `stderr` and exits with `1`.

## Patched zsh

We carry a small patch to `Src/exec.c` (see `patches/zsh-exec-wrapper.patch`) that adds support for `EXEC_WRAPPER`. The patch applies to `77045ef899e53b9598bebc5a41db93a548a40ca6` from https://git.code.sf.net/p/zsh/code. To rebuild manually:

```bash
git clone https://git.code.sf.net/p/zsh/code
git checkout 77045ef899e53b9598bebc5a41db93a548a40ca6
git apply /path/to/patches/zsh-exec-wrapper.patch
./Util/preconfig
./configure
make -j"$(nproc)"
```

Release artifacts are built by `.github/workflows/rust-release-zsh.yml` when a
`atlas-engine-zsh-vX.Y.Z` tag is pushed. When the zsh commit or patch changes, publish
the next version tag and update the checked-in DotSlash manifests to use the new
release.
