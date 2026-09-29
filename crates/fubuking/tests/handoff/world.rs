//! A person's machine in a temporary directory: a home where the fakes are
//! the only CLIs in reach, a project folder, and `fubuking` started on a
//! terminal of its own, the way a person starts it.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::fake::{self, Call};

/// How long a question or an exit may take. Well under the minute a fake
/// waits to be stopped, so a stop that never comes fails the test.
const PATIENCE: Duration = Duration::from_secs(20);

pub struct World {
    _root: tempfile::TempDir,
    pub home: PathBuf,
    pub project: PathBuf,
    bin: PathBuf,
    state: PathBuf,
}

impl World {
    /// An empty home and project, with the fakes as `claude` and `codex` on
    /// `PATH`: this binary under those names.
    pub fn new() -> Self {
        let root = tempfile::tempdir().expect("temporary directory");
        // macOS hands out `/var/...`, a link to `/private/var/...`; the CLIs
        // record the resolved path.
        let base = root.path().canonicalize().expect("temporary directory");
        let (home, project, bin, state) = (base.join("home"), base.join("project"), base.join("bin"), base.join("state"));
        for dir in [&home, &project, &bin, &state] {
            std::fs::create_dir_all(dir).expect("world folder");
        }
        let this = std::env::current_exe().expect("test binary");
        for name in ["claude", "codex"] {
            std::os::unix::fs::symlink(&this, bin.join(name)).expect("fake CLI");
        }
        Self { _root: root, home, project, bin, state }
    }

    /// The default login's CLI homes.
    pub fn claude_home(&self) -> PathBuf {
        self.home.join(".claude")
    }

    pub fn codex_home(&self) -> PathBuf {
        self.home.join(".codex")
    }

    /// Say how the runs of the account whose CLI home is `home` end.
    pub fn plan(&self, home: &Path, plan: &str) {
        std::fs::create_dir_all(home).expect("CLI home");
        std::fs::write(home.join(fake::PLAN), plan).expect("plan");
    }

    /// Add an account with `fubuking login` and return its CLI home. The
    /// login's own run is not kept among the calls.
    pub fn login(&self, provider: &str, name: &str) -> PathBuf {
        let status = self
            .command(&["login", provider, name])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("fubuking login");
        assert!(status.success(), "fubuking login {provider} {name}: {status}");
        let home = self.calls().pop().expect("the login ran the CLI").home;
        std::fs::remove_file(self.state.join("calls.jsonl")).expect("clearing the calls");
        home
    }

    /// `fubuking <args>` on a terminal.
    pub fn run(&self, args: &[&str]) -> Terminal {
        Terminal::start(self.command(args))
    }

    /// `fubuking <args>` with no terminal, as a script runs it: its exit
    /// code, what it printed and what it said on stderr.
    pub fn output(&self, args: &[&str]) -> (i32, String, String) {
        let out = self.command(args).stdin(Stdio::null()).output().expect("running fubuking");
        let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
        (out.status.code().unwrap_or(-1), text(&out.stdout), text(&out.stderr))
    }

    /// Make `program --version` say `version`.
    pub fn set_version(&self, program: &str, version: &str) {
        std::fs::write(self.state.join(format!("{program}-version")), version).expect("version");
    }

    /// Every run of a fake CLI so far, in order.
    pub fn calls(&self) -> Vec<Call> {
        std::fs::read_to_string(self.state.join("calls.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).expect("call"))
            .collect()
    }

    /// The one Claude Code session `home` holds for the project.
    pub fn claude_session(&self, home: &Path) -> PathBuf {
        let dir = home.join("projects").join(fake::encode(&self.project.to_string_lossy()));
        only(sessions(&dir))
    }

    /// The one Codex rollout `home` holds.
    pub fn codex_session(&self, home: &Path) -> PathBuf {
        only(sessions(&home.join("sessions")))
    }

    /// Nothing of this process's environment reaches `fubuking`: not the
    /// real CLIs, their homes or their variables.
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fubuking"));
        command
            .args(args)
            .current_dir(&self.project)
            .env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", self.bin.display()))
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("TERM", "xterm-256color")
            .env(fake::STATE, &self.state);
        command
    }
}

/// Every `.jsonl` file under `dir`.
fn sessions(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            found.extend(sessions(&path));
        } else if path.extension().is_some_and(|ext| ext == "jsonl") {
            found.push(path);
        }
    }
    found
}

fn only(mut paths: Vec<PathBuf>) -> PathBuf {
    assert_eq!(paths.len(), 1, "one session, not {paths:?}");
    paths.remove(0)
}

/// A process on a pseudo-terminal that is its controlling terminal, as a
/// program a person starts has: questions reach it, and `/dev/tty` opens.
pub struct Terminal {
    child: Child,
    input: File,
    output: Arc<Mutex<Vec<u8>>>,
    reader: JoinHandle<()>,
}

impl Terminal {
    fn start(mut command: Command) -> Self {
        let (mut primary, mut secondary) = (0, 0);
        // SAFETY: both out-pointers are valid for the call; the name, the
        // settings and the size may be null.
        let opened = unsafe {
            libc::openpty(&mut primary, &mut secondary, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut())
        };
        assert_eq!(opened, 0, "openpty: {}", std::io::Error::last_os_error());
        // SAFETY: openpty returned two open descriptors nothing else owns.
        let (primary, secondary) = unsafe { (OwnedFd::from_raw_fd(primary), OwnedFd::from_raw_fd(secondary)) };
        let side = || Stdio::from(secondary.try_clone().expect("terminal"));
        command.stdin(side()).stdout(side()).stderr(side());
        // SAFETY: between fork and exec the child makes only
        // async-signal-safe calls: a new session, then the terminal on its
        // standard input as that session's controlling terminal.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn().expect("starting fubuking");
        // Keep no copy of the child's side, so reading ends once every
        // process on the terminal is gone.
        drop(command);
        drop(secondary);
        let input = File::from(primary.try_clone().expect("terminal"));
        let output = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&output);
        let mut screen = File::from(primary);
        let reader = std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            // Linux answers EIO rather than end of file once the other side
            // is closed; either ends it.
            while let Ok(read @ 1..) = screen.read(&mut chunk) {
                sink.lock().expect("screen").extend_from_slice(&chunk[..read]);
            }
        });
        Self { child, input, output, reader }
    }

    /// Everything the terminal has shown so far.
    pub fn screen(&self) -> String {
        String::from_utf8_lossy(&self.output.lock().expect("screen")).into_owned()
    }

    /// Wait until the terminal shows `text`.
    pub fn expect(&self, text: &str) {
        let deadline = Instant::now() + PATIENCE;
        while !self.screen().contains(text) {
            assert!(Instant::now() < deadline, "no {text:?} on the terminal:\n{}", self.screen());
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Type `line` and Enter.
    pub fn answer(&mut self, line: &str) {
        self.input.write_all(format!("{line}\n").as_bytes()).expect("typing");
    }

    /// Wait for `fubuking` to exit: its exit code and the whole screen.
    pub fn finish(mut self) -> (i32, String) {
        let deadline = Instant::now() + PATIENCE;
        let status = loop {
            if let Some(status) = self.child.try_wait().expect("waiting for fubuking") {
                break status;
            }
            if Instant::now() > deadline {
                let group = libc::pid_t::try_from(self.child.id()).expect("pid");
                // SAFETY: a signal to the session `fubuking` leads, and only it.
                unsafe { libc::kill(-group, libc::SIGKILL) };
                panic!("fubuking did not exit:\n{}", self.screen());
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while !self.reader.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        (status.code().unwrap_or(-1), self.screen())
    }
}
