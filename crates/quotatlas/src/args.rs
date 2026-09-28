//! The user's own options, carried from the first run into a resumed or
//! handed-off session.
//!
//! A resumed session starts from `--resume <id>` or `resume <id>`, so options
//! given the first time, such as the model or the permission mode, would be
//! lost. Which words are options and how many values each takes is read from
//! the CLI's own `--help` when a handoff happens, so the table cannot fall
//! behind the installed CLI. An option the help does not list is taken to have
//! no value.

use std::collections::HashMap;
use std::ffi::OsString;

/// How many values an option takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Takes {
    Nothing,
    One,
    /// `[value]`: the next word, unless it is an option.
    Optional,
    /// `<values...>`: every word up to the next option.
    Many,
}

/// Every option a CLI's help lists, by each of its names.
#[derive(Debug, Default)]
pub struct Options(HashMap<String, Takes>);

impl Options {
    /// Read the option lines of `help`: commander's `--add-dir <dirs...>  text`
    /// and clap's `-m, --model <MODEL>`. Continuation lines, indented deeper,
    /// are skipped even when they start with an option name.
    pub fn from_help(help: &str) -> Self {
        let mut options = HashMap::new();
        for line in help.lines() {
            let indent = line.len() - line.trim_start().len();
            let spec = line.trim_start();
            if indent > 6 || !spec.starts_with('-') {
                continue;
            }
            let spec = spec.split("  ").next().unwrap_or(spec);
            let words: Vec<&str> = spec.split_whitespace().collect();
            let takes = match words.iter().find(|w| w.starts_with('<') || w.starts_with('[')) {
                None => Takes::Nothing,
                Some(value) if value.contains("...") => Takes::Many,
                Some(value) if value.starts_with('[') => Takes::Optional,
                Some(_) => Takes::One,
            };
            for word in words.iter().filter(|w| w.starts_with('-')) {
                options.insert(word.trim_end_matches(',').to_string(), takes);
            }
        }
        Self(options)
    }

    /// The options `program` (or its `subcommand`) lists in its help; empty
    /// when the help cannot be read.
    pub fn of(program: &str, subcommand: Option<&str>) -> Self {
        atlas_process::command(program)
            .args(subcommand)
            .arg("--help")
            .output()
            .map(|out| Self::from_help(&String::from_utf8_lossy(&out.stdout)))
            .unwrap_or_default()
    }

    /// Both tables, `other` winning where they disagree.
    pub fn merged(mut self, other: Self) -> Self {
        self.0.extend(other.0);
        self
    }

    fn takes(&self, name: &str) -> Takes {
        self.0.get(name).copied().unwrap_or(Takes::Nothing)
    }
}

/// The options in `args` with their values, in order: positional words
/// (prompts, subcommands) and the options named in `drop` are left out.
/// Everything after `--` is positional.
pub fn options_only(args: &[OsString], options: &Options, drop: &[&str]) -> Vec<OsString> {
    let is_option = |word: &OsString| word.to_string_lossy().starts_with('-') && word != "-";
    let mut kept = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let word = args[i].to_string_lossy().into_owned();
        if word == "--" {
            break;
        }
        if !is_option(&args[i]) {
            i += 1;
            continue;
        }
        let (name, inline) = match word.split_once('=') {
            Some((name, _)) => (name.to_string(), true),
            None => (word.clone(), false),
        };
        let mut group = vec![args[i].clone()];
        i += 1;
        if !inline {
            match options.takes(&name) {
                Takes::Nothing => {}
                Takes::One => {
                    if let Some(value) = args.get(i) {
                        group.push(value.clone());
                        i += 1;
                    }
                }
                Takes::Optional => {
                    if args.get(i).is_some_and(|next| !is_option(next)) {
                        group.push(args[i].clone());
                        i += 1;
                    }
                }
                Takes::Many => {
                    while args.get(i).is_some_and(|next| !is_option(next)) {
                        group.push(args[i].clone());
                        i += 1;
                    }
                }
            }
        }
        if !drop.contains(&name.as_str()) {
            kept.extend(group);
        }
    }
    kept
}

/// The first positional word: the subcommand, when the CLI has one.
pub fn first_word(args: &[OsString], options: &Options) -> Option<String> {
    let all: Vec<OsString> = args.iter().take_while(|w| *w != "--").cloned().collect();
    let flags = options_only(&all, options, &[]);
    let mut flags = flags.iter().peekable();
    for word in &all {
        if flags.peek() == Some(&word) {
            flags.next();
        } else {
            return Some(word.to_string_lossy().into_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE_HELP: &str = "\
Options:
  --add-dir <directories...>            Additional directories to allow tool
  --allowedTools, --allowed-tools <tools...>
  -c, --continue                        Continue the most recent conversation in
                                        --resume <session-id>, continues that
  -d, --debug [filter]                  Enable debug mode with optional category
  --model <model>                       Model for the current session. Provide
  -p, --print                           Print response and exit (useful for
  -r, --resume [value]                  Resume a conversation by session ID, or
  --dangerously-skip-permissions        Bypass all permission checks.
";

    const CODEX_HELP: &str = "\
Options:
  -c, --config <key=value>
          Override a configuration value
      --enable <FEATURE>
  -i, --image <FILE>...
  -m, --model <MODEL>
      --search
";

    fn words(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn help_lines_give_each_name_its_arity() {
        let claude = Options::from_help(CLAUDE_HELP);
        assert_eq!(claude.takes("--add-dir"), Takes::Many);
        assert_eq!(claude.takes("--allowed-tools"), Takes::Many);
        assert_eq!(claude.takes("--allowedTools"), Takes::Many);
        assert_eq!(claude.takes("-r"), Takes::Optional);
        assert_eq!(claude.takes("--model"), Takes::One);
        assert_eq!(claude.takes("-p"), Takes::Nothing);
        // A description line that happens to start with an option is not one.
        assert_eq!(claude.takes("--resume"), Takes::Optional);
        let codex = Options::from_help(CODEX_HELP);
        assert_eq!(codex.takes("-c"), Takes::One);
        assert_eq!(codex.takes("-i"), Takes::Many);
        assert_eq!(codex.takes("--search"), Takes::Nothing);
    }

    #[test]
    fn prompts_and_session_selectors_are_dropped_and_values_kept() {
        let claude = Options::from_help(CLAUDE_HELP);
        let args = words(&["fix the bug", "--model", "opus", "-r", "abc", "--add-dir", "a", "b", "--dangerously-skip-permissions", "-c"]);
        assert_eq!(
            options_only(&args, &claude, &["-r", "--resume", "-c", "--continue"]),
            words(&["--model", "opus", "--add-dir", "a", "b", "--dangerously-skip-permissions"])
        );
        let inline = words(&["--model=sonnet", "--", "--not-an-option"]);
        assert_eq!(options_only(&inline, &claude, &[]), words(&["--model=sonnet"]));
    }

    #[test]
    fn the_subcommand_is_the_first_word_no_option_claims() {
        let codex = Options::from_help(CODEX_HELP);
        assert_eq!(first_word(&words(&["-m", "gpt", "exec", "task"]), &codex).as_deref(), Some("exec"));
        assert_eq!(first_word(&words(&["--search"]), &codex), None);
    }
}
