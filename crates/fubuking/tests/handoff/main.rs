//! End to end: the `fubuking` binary on a terminal, as a person runs it,
//! with fake `claude` and `codex` CLIs on `PATH`. Each test drives a whole
//! handoff: the limit, the stop of a CLI still open, the question, the copy,
//! and the session resumed, or started from a brief, on the next account.
//!
//! The fakes write their sessions where the real CLIs do, from real records
//! with ids, paths and text replaced (`fixtures/`): Claude Code 2.1.283's
//! request and reply and 2.1.281's limit, Codex 0.157.1's header, messages
//! and turn end and 0.152.1's limit, and each CLI's own `--help` (Claude Code
//! 2.1.283, Codex 0.157.1). When a CLI changes its format, refresh these and
//! the tests say whether FubuKing still follows.
//!
//! The fakes are this binary started as `claude` and `codex`, so it runs
//! without the test harness and picks its part by the name it was started
//! as. Unix only: the handoff asks on a terminal, and on Windows on a
//! console, which this does not drive.

#[cfg(unix)]
mod fake;
#[cfg(unix)]
mod world;

fn main() {
    #[cfg(unix)]
    unix::main();
}

#[cfg(unix)]
mod unix {
    use std::path::Path;
    use std::time::Instant;

    use fubuking::run::CONTINUE_PROMPT;

    use crate::fake::{self, LIMIT, LOST, WORKING};
    use crate::world::World;

    const TASK: &str = "Fix the flaky retry test";
    const TESTS: [(&str, fn()); 7] = [
        ("claude_moves_a_stopped_session_to_the_next_account", claude_moves_a_stopped_session_to_the_next_account),
        ("claude_print_mode_resumes_in_print_mode", claude_print_mode_resumes_in_print_mode),
        ("claude_with_no_account_left_goes_on_in_codex_from_a_brief", claude_with_no_account_left_goes_on_in_codex_from_a_brief),
        ("codex_moves_a_stopped_session_to_the_next_account", codex_moves_a_stopped_session_to_the_next_account),
        ("codex_with_no_account_left_goes_on_in_claude_from_a_brief", codex_with_no_account_left_goes_on_in_claude_from_a_brief),
        ("a_session_the_next_account_cannot_find_starts_there_from_a_brief", a_session_the_next_account_cannot_find_starts_there_from_a_brief),
        ("declining_leaves_the_session_where_it_stopped", declining_leaves_the_session_where_it_stopped),
    ];

    pub fn main() {
        let started_as = std::env::args_os()
            .next()
            .and_then(|name| Path::new(&name).file_name().map(|file| file.to_string_lossy().into_owned()))
            .unwrap_or_default();
        match started_as.as_str() {
            "claude" => std::process::exit(fake::claude()),
            "codex" => std::process::exit(fake::codex()),
            _ => std::process::exit(run_tests()),
        }
    }

    /// Run the tests whose names contain a word given on the command line, or
    /// all of them, side by side; each has a world of its own.
    fn run_tests() -> i32 {
        let filters: Vec<String> = std::env::args().skip(1).filter(|word| !word.starts_with('-')).collect();
        let chosen: Vec<(&str, fn())> = TESTS
            .into_iter()
            .filter(|(name, _)| filters.is_empty() || filters.iter().any(|filter| name.contains(filter.as_str())))
            .collect();
        if std::env::args().any(|word| word == "--list") {
            for (name, _) in &chosen {
                println!("{name}: test");
            }
            return 0;
        }
        println!("\nrunning {} tests", chosen.len());
        let started = Instant::now();
        let results: Vec<(&str, bool)> = std::thread::scope(|scope| {
            // Every test starts before the first is waited for.
            let mut running = Vec::new();
            for &(name, test) in &chosen {
                running.push((name, scope.spawn(test)));
            }
            running.into_iter().map(|(name, test)| (name, test.join().is_ok())).collect()
        });
        for (name, passed) in &results {
            println!("test {name} ... {}", if *passed { "ok" } else { "FAILED" });
        }
        let failed = results.iter().filter(|(_, passed)| !passed).count();
        println!(
            "\ntest result: {}. {} passed; {failed} failed; 0 ignored; 0 measured; {} filtered out; finished in {:.2}s\n",
            if failed == 0 { "ok" } else { "FAILED" },
            results.len() - failed,
            TESTS.len() - chosen.len(),
            started.elapsed().as_secs_f64()
        );
        i32::from(failed > 0)
    }

    /// The first account runs out while Claude Code is open. FubuKing stops
    /// it (the fake would otherwise stay open for a minute, past the test's
    /// patience), asks, copies the session to the next account and resumes it
    /// there with the first run's options.
    fn claude_moves_a_stopped_session_to_the_next_account() {
        let world = World::new();
        let second = world.login("claude", "second");
        world.plan(&world.claude_home(), LIMIT);
        let mut terminal = world.run(&["claude", "--model", "opus", TASK]);
        terminal.expect("resume this session on claude account `second`? [Y/n]");
        terminal.answer("");
        let (code, screen) = terminal.finish();
        assert_eq!(code, 0, "{screen}");
        assert!(screen.contains("stopped on its usage limit: You've hit your session limit"), "{screen}");

        let calls = world.calls();
        let [first, resumed] = &calls[..] else { panic!("{calls:#?}") };
        let original = world.claude_session(&world.claude_home());
        let id = stem(&original);
        assert_eq!(first.home, world.claude_home());
        assert!(has(&first.args, &["--model", "opus", TASK]), "{first:?}");
        assert_eq!(resumed.home, second);
        assert!(has(&resumed.args, &["--model", "opus"]), "{resumed:?}");
        // FubuKing's own options go last before `--`, where they cannot take the prompt.
        assert!(has(&resumed.args, &["--resume", &id]), "{resumed:?}");
        assert!(ends(&resumed.args, &["--", CONTINUE_PROMPT]), "{resumed:?}");
        assert!(!resumed.args.iter().any(|word| word == TASK), "{resumed:?}");

        let copy = world.claude_session(&second);
        assert_eq!(copy.strip_prefix(&second).ok(), original.strip_prefix(world.claude_home()).ok());
        let (before, after) = (read(&original), read(&copy));
        assert!(after.starts_with(&before) && after.contains(fake::DONE), "the copy carries the session on:\n{after}");
    }

    /// `claude -p` ends on the limit by itself, and the resume runs in print
    /// mode too.
    fn claude_print_mode_resumes_in_print_mode() {
        let world = World::new();
        let second = world.login("claude", "second");
        world.plan(&world.claude_home(), LIMIT);
        let mut terminal = world.run(&["claude", "-p", TASK]);
        terminal.expect("resume this session on claude account `second`? [Y/n]");
        terminal.answer("y");
        let (code, screen) = terminal.finish();
        assert_eq!(code, 0, "{screen}");

        let calls = world.calls();
        let [_, resumed] = &calls[..] else { panic!("{calls:#?}") };
        let id = stem(&world.claude_session(&world.claude_home()));
        assert_eq!(resumed.home, second);
        assert!(has(&resumed.args, &["-p"]), "{resumed:?}");
        // FubuKing's own options go last before `--`, where they cannot take the prompt.
        assert!(has(&resumed.args, &["--resume", &id]), "{resumed:?}");
        assert!(ends(&resumed.args, &["--", CONTINUE_PROMPT]), "{resumed:?}");
    }

    /// With no other Claude account, Codex takes over from a brief of the
    /// session: what was asked and last answered, and where to look.
    fn claude_with_no_account_left_goes_on_in_codex_from_a_brief() {
        let world = World::new();
        world.plan(&world.claude_home(), LIMIT);
        let mut terminal = world.run(&["claude", TASK]);
        terminal.expect("Continue in Codex on codex account `default` with a brief of this session? [Y/n]");
        terminal.answer("");
        let (code, screen) = terminal.finish();
        assert_eq!(code, 0, "{screen}");

        let calls = world.calls();
        let [first, codex] = &calls[..] else { panic!("{calls:#?}") };
        assert_eq!((first.program.as_str(), codex.program.as_str()), ("claude", "codex"));
        assert_eq!(codex.home, world.codex_home());
        assert!(!codex.args.iter().any(|word| word == "resume"), "{codex:?}");
        let brief = codex.args.last().expect("a brief");
        for part in [TASK, WORKING, "memory_briefing"] {
            assert!(brief.contains(part), "{part:?} missing from the brief:\n{brief}");
        }
    }

    /// The same for Codex, whose rollout also carries the resume path.
    fn codex_moves_a_stopped_session_to_the_next_account() {
        let world = World::new();
        let side = world.login("codex", "side");
        world.plan(&world.codex_home(), LIMIT);
        let mut terminal = world.run(&["codex", "-m", "gpt-test", TASK]);
        terminal.expect("resume this session on codex account `side`? [Y/n]");
        terminal.answer("");
        let (code, screen) = terminal.finish();
        assert_eq!(code, 0, "{screen}");
        assert!(screen.contains("stopped on its usage limit: You've hit your usage limit"), "{screen}");

        let calls = world.calls();
        let [first, resumed] = &calls[..] else { panic!("{calls:#?}") };
        let original = world.codex_session(&world.codex_home());
        let name = stem(&original);
        // `rollout-<local time>-<thread id>`
        let id = &name[name.len() - 36..];
        assert!(has(&first.args, &["-m", "gpt-test", TASK]), "{first:?}");
        assert_eq!(resumed.home, side);
        assert!(ends(&resumed.args, &["-m", "gpt-test", "resume", id, CONTINUE_PROMPT]), "{resumed:?}");

        let copy = world.codex_session(&side);
        assert_eq!(copy.strip_prefix(&side).ok(), original.strip_prefix(world.codex_home()).ok());
        assert!(read(&copy).starts_with(&read(&original)), "the copy carries the session on");
    }

    /// Claude Code takes over from Codex. Codex records its instructions and
    /// environment as user messages; the brief quotes only what the person
    /// asked.
    fn codex_with_no_account_left_goes_on_in_claude_from_a_brief() {
        let world = World::new();
        world.plan(&world.codex_home(), LIMIT);
        let mut terminal = world.run(&["codex", TASK]);
        terminal.expect("Continue in Claude Code on claude account `default` with a brief of this session? [Y/n]");
        terminal.answer("");
        let (code, screen) = terminal.finish();
        assert_eq!(code, 0, "{screen}");

        let calls = world.calls();
        let [_, claude] = &calls[..] else { panic!("{calls:#?}") };
        assert_eq!(claude.program, "claude");
        let brief = claude.args.last().expect("a brief");
        assert!(ends(&claude.args, &["--", brief.as_str()]), "{claude:?}");
        for part in [TASK, WORKING] {
            assert!(brief.contains(part), "{part:?} missing from the brief:\n{brief}");
        }
        for part in ["AGENTS.md", "environment_context"] {
            assert!(!brief.contains(part), "{part:?} in the brief:\n{brief}");
        }
    }

    /// A resume that finds no session (the CLI's layout changed) starts the
    /// account over from a brief, after asking.
    fn a_session_the_next_account_cannot_find_starts_there_from_a_brief() {
        let world = World::new();
        let second = world.login("claude", "second");
        world.plan(&world.claude_home(), LIMIT);
        world.plan(&second, LOST);
        let mut terminal = world.run(&["claude", TASK]);
        terminal.expect("resume this session on claude account `second`? [Y/n]");
        terminal.answer("");
        terminal.expect("did not resume session");
        terminal.answer("");
        let (code, screen) = terminal.finish();
        assert_eq!(code, 0, "{screen}");

        let calls = world.calls();
        let [_, resume, fresh] = &calls[..] else { panic!("{calls:#?}") };
        assert!(resume.args.iter().any(|word| word == "--resume"), "{resume:?}");
        assert_eq!(fresh.home, second);
        assert!(!fresh.args.iter().any(|word| word == "--resume"), "{fresh:?}");
        let brief = fresh.args.last().expect("a brief");
        assert!(brief.contains(TASK), "{brief}");
    }

    /// No means no: nothing is copied and the stopped CLI's exit is the run's.
    fn declining_leaves_the_session_where_it_stopped() {
        let world = World::new();
        let second = world.login("claude", "second");
        world.plan(&world.claude_home(), LIMIT);
        let mut terminal = world.run(&["claude", TASK]);
        terminal.expect("resume this session on claude account `second`? [Y/n]");
        terminal.answer("n");
        let (code, screen) = terminal.finish();
        assert_ne!(code, 0, "{screen}");
        assert_eq!(world.calls().len(), 1);
        assert!(!second.join("projects").exists(), "nothing is copied");
    }

    /// Whether `words` holds `run` in a row.
    fn has(words: &[String], run: &[&str]) -> bool {
        words.windows(run.len()).any(|window| window.iter().zip(run).all(|(word, want)| word == want))
    }

    fn ends(words: &[String], tail: &[&str]) -> bool {
        words.len() >= tail.len() && has(&words[words.len() - tail.len()..], tail)
    }

    fn stem(path: &Path) -> String {
        path.file_stem().expect("a file").to_string_lossy().into_owned()
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).expect("session")
    }
}
