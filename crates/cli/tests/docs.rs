//! The skill docs, checked against the binary rather than by eye.
//!
//! `skill/taskforge/` is what an agent reads to learn this tool, so a wrong claim there is a
//! defect with the same consequences as a wrong line of code — the agent does the wrong thing
//! and nothing complains. These tests exist because that is exactly what happened: a hand-written
//! "legal from" column claimed three transitions that the code refuses, and a documented example
//! called a flag (`reject --text`) that does not exist. Both read fine.
//!
//! The rule they encode: **no factual claim about the CLI is verified by reading it.** Either a
//! test derives the claim from the code or runs the command, or the doc should not make it.

use std::collections::BTreeSet;
use std::process::Command;
use taskforge_core::model::TaskStatus;
use taskforge_core::transition::can_transition;

fn skill_dir() -> std::path::PathBuf {
    // CARGO_MANIFEST_DIR is crates/cli; the skill lives at the workspace root.
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skill/taskforge")
}

fn doc(name: &str) -> String {
    let path = skill_dir().join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// Whether `haystack` contains `token` as a whole word.
///
/// A plain `contains` is not enough: `INVALID_PARENT` is a substring of `INVALID_PARENT_TYPO`, so
/// a renamed code would still look documented. Verified by the guard-probe that this test file
/// exists to satisfy.
fn mentions_token(haystack: &str, token: &str) -> bool {
    haystack.match_indices(token).any(|(i, _)| {
        let boundary =
            |c: Option<char>| !c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        boundary(haystack[..i].chars().next_back())
            && boundary(haystack[i + token.len()..].chars().next())
    })
}

/// Every `task` subcommand the binary exposes, from its own `--help`.
fn real_commands() -> Vec<String> {
    subcommands_of(&["task", "--help"])
}

/// Top-level commands: `init`, `owner`, `task`, `workspace`, `doctor`.
///
/// Checked separately because `real_commands` only reads `task --help`, so for the whole life of
/// this crate every top-level command has been exempt from the documentation test by accident.
fn top_level_commands() -> Vec<String> {
    subcommands_of(&["--help"])
}

fn subcommands_of(args: &[&str]) -> Vec<String> {
    let out = Command::new(env!("CARGO_BIN_EXE_taskforge"))
        .args(args)
        .output()
        .expect("binary runs");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        // clap indents subcommands by two spaces; anything further is continued help text.
        .filter_map(|l| {
            let rest = l.strip_prefix("  ")?;
            if rest.starts_with(char::is_whitespace) {
                return None;
            }
            let name = rest.split_whitespace().next()?;
            (name != "help" && name.chars().all(|c| c.is_ascii_lowercase() || c == '-'))
                .then(|| name.to_string())
        })
        .collect()
}

/// The target status each transition command moves a task to. The pairing the docs describe.
const COMMAND_TARGETS: &[(&str, TaskStatus)] = &[
    ("start", TaskStatus::Running),
    ("request-review", TaskStatus::InReview),
    ("reject", TaskStatus::ChangesRequested),
    ("merge", TaskStatus::Merged),
    ("accept", TaskStatus::Done),
    ("complete", TaskStatus::Done),
    ("pending", TaskStatus::Pending),
    ("wait", TaskStatus::WaitingOnSchedule),
    ("block", TaskStatus::Stuck),
    ("fail", TaskStatus::Failed),
    ("cancel", TaskStatus::Cancelled),
];

/// Parse the "Legal from" column of the transition table for one command.
///
/// Returns `None` when the table has no row for it, which is itself a failure the caller reports.
fn documented_sources(reference: &str, command: &str) -> Option<BTreeSet<String>> {
    let prefix = format!("| `task {command}` |");
    let row = reference.lines().find(|l| l.starts_with(&prefix))?;
    let cell = row.split('|').nth(2)?;
    // Match whole status names only: "in-review" must not also count as a match for "review",
    // and "waiting-on-schedule" contains no other status name by accident.
    Some(
        TaskStatus::ALL
            .iter()
            .filter(|s| {
                let name = s.as_str();
                cell.match_indices(name).any(|(i, _)| {
                    let before = cell[..i].chars().next_back();
                    let after = cell[i + name.len()..].chars().next();
                    let boundary =
                        |c: Option<char>| !c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '-');
                    boundary(before) && boundary(after)
                })
            })
            .map(|s| s.as_str().to_string())
            .collect(),
    )
}

#[test]
fn the_documented_transition_table_matches_the_status_machine() {
    // Compared against `can_transition` rather than against the binary: the core suite already
    // pins `can_transition` to an independently written spec list, so doc -> code -> spec is a
    // complete chain, and this stays a pure-function check instead of 600 process spawns.
    let reference = doc("reference.md");
    let mut wrong = Vec::new();

    for (command, target) in COMMAND_TARGETS {
        let Some(documented) = documented_sources(&reference, command) else {
            wrong.push(format!(
                "`task {command}` has no row in the transition table"
            ));
            continue;
        };
        let actual: BTreeSet<String> = TaskStatus::ALL
            .iter()
            .filter(|from| can_transition(**from, *target))
            .map(|s| s.as_str().to_string())
            .collect();
        if documented != actual {
            let claimed: Vec<&String> = documented.difference(&actual).collect();
            let omitted: Vec<&String> = actual.difference(&documented).collect();
            wrong.push(format!(
                "`task {command}` (-> {target}):\n     docs say : {documented:?}\n     code says: {actual:?}\
                 \n     documented but refused: {claimed:?}\n     legal but undocumented: {omitted:?}"
            ));
        }
    }

    assert!(
        wrong.is_empty(),
        "reference.md disagrees with the transition table in {} place(s):\n  {}",
        wrong.len(),
        wrong.join("\n  ")
    );
}

#[test]
fn every_command_the_binary_has_is_named_in_the_reference() {
    let reference = doc("reference.md");
    let undocumented: Vec<String> = real_commands()
        .into_iter()
        // Commands appear either as `taskforge task x` in an example or as `task x` in a table.
        // Whole-word, or documenting only `set-review-required` would also satisfy `set-review`.
        .filter(|c| !mentions_token(&reference, &format!("task {c}")))
        .collect();
    assert!(
        undocumented.is_empty(),
        "commands the binary has and reference.md never mentions: {undocumented:?}"
    );
}

#[test]
fn every_top_level_command_is_named_in_the_reference_too() {
    let reference = doc("reference.md");
    let undocumented: Vec<String> = top_level_commands()
        .into_iter()
        .filter(|c| !mentions_token(&reference, &format!("taskforge {c}")))
        .collect();
    assert!(
        undocumented.is_empty(),
        "top-level commands reference.md never mentions: {undocumented:?}"
    );
}

#[test]
fn every_code_the_cli_can_emit_is_documented() {
    // Sourced from main.rs rather than from a hand-kept list, so a new code added without a doc
    // entry fails here instead of shipping undocumented.
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs"),
    )
    .expect("reading main.rs");
    let reference = doc("reference.md");

    // Environment-variable names look exactly like response codes and are not response codes.
    // Derived from the source instead of a hand-kept allowlist: the allowlist version failed the
    // first time a new `env!` was added, which is the same "someone has to remember" that this
    // whole test exists to remove.
    let env_names: BTreeSet<&str> = ["env!(\"", "env::var(\""]
        .iter()
        .flat_map(|pat| {
            source.match_indices(pat).filter_map(|(i, m)| {
                let rest = &source[i + m.len()..];
                rest.find('"').map(|end| &rest[..end])
            })
        })
        .collect();

    let mut missing = Vec::new();
    for raw in source.split('"').skip(1).step_by(2) {
        let is_code = raw.len() > 3
            && raw
                .chars()
                .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit())
            && raw.contains('_');
        if is_code && !env_names.contains(raw) && !mentions_token(&reference, raw) {
            missing.push(raw.to_string());
        }
    }
    missing.sort();
    missing.dedup();
    assert!(
        missing.is_empty(),
        "codes the CLI emits that reference.md does not document: {missing:?}"
    );
}

#[test]
fn every_flag_the_docs_show_exists_on_that_command() {
    // Catches the `reject --text` class: a flag that reads perfectly and does not exist. Checked
    // against `--help` rather than by running the line, so synopsis notation (`[--status <s>]`)
    // is covered too — those lines are not runnable but their flag names are still claims.
    let mut broken = Vec::new();
    for file in ["SKILL.md", "reference.md", "workflows.md"] {
        for invocation in documented_invocations(&doc(file)) {
            let help = Command::new(env!("CARGO_BIN_EXE_taskforge"))
                .args(["task", &invocation.command, "--help"])
                .output()
                .expect("binary runs");
            let help = String::from_utf8_lossy(&help.stdout).to_string();
            for flag in &invocation.flags {
                if !help.contains(flag) {
                    broken.push(format!(
                        "{file}: `task {} {flag}` — no such flag\n     {}",
                        invocation.command, invocation.line
                    ));
                }
            }
        }
    }
    assert!(
        broken.is_empty(),
        "documented flags that do not exist:\n  {}",
        broken.join("\n  ")
    );
}

#[test]
fn every_runnable_documented_example_is_valid_usage() {
    let root = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| -> String {
        let out = Command::new(env!("CARGO_BIN_EXE_taskforge"))
            .args(args)
            .env("TASKFORGE_ROOT", root.path())
            .output()
            .expect("binary runs");
        String::from_utf8_lossy(&out.stderr).to_string()
    };
    run(&["init", "--json"]);
    for (name, kind) in [("aaron", "human"), ("agent", "agent"), ("rev", "human")] {
        run(&["owner", "add", "--name", name, "--type", kind, "--json"]);
    }

    let mut broken = Vec::new();
    let mut checked = 0;
    for file in ["SKILL.md", "reference.md", "workflows.md"] {
        for invocation in documented_invocations(&doc(file)) {
            if !invocation.runnable {
                continue;
            }
            checked += 1;
            let args: Vec<&str> = invocation.line.split_whitespace().skip(1).collect();
            let stderr = run(&args);
            // Only usage errors matter. A command failing on *state* (TASK_NOT_FOUND, an illegal
            // transition) is fine — the docs are not a runnable script, and asserting otherwise
            // would force every example into one contrived order.
            if stderr.contains("unexpected argument")
                || stderr.contains("invalid value")
                || stderr.contains("unrecognized")
                || stderr.contains("the following required arguments")
            {
                broken.push(format!(
                    "{file}: {}\n     {}",
                    invocation.line,
                    stderr.lines().next().unwrap_or_default()
                ));
            }
        }
    }
    assert!(
        broken.is_empty(),
        "documented commands that are not valid usage:\n  {}",
        broken.join("\n  ")
    );
    assert!(
        checked >= 20,
        "only {checked} runnable examples found — the extractor has probably stopped matching"
    );
}

struct Invocation {
    command: String,
    flags: Vec<String>,
    line: String,
    /// Whether the line is literal enough to execute. Synopsis notation (`[optional]`,
    /// `<placeholder>`), shell substitution and pipes are illustrative, not runnable.
    runnable: bool,
}

/// `taskforge task …` invocations from a markdown file, joined across `\` continuations.
fn documented_invocations(text: &str) -> Vec<Invocation> {
    let mut out = Vec::new();
    let mut pending: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        let mut current = match pending.take() {
            Some(prefix) => format!("{prefix} {line}"),
            None if line.starts_with("taskforge task ") => line.to_string(),
            None => continue,
        };
        if let Some(stripped) = current.strip_suffix('\\') {
            pending = Some(stripped.trim_end().to_string());
            continue;
        }
        // Trailing prose comments in an example are not arguments.
        if let Some(hash) = current.find(" #") {
            current.truncate(hash);
        }
        let current = current.trim().to_string();
        let Some(command) = current.split_whitespace().nth(2) else {
            continue;
        };
        let notation = current.contains('[') || current.contains('<');
        let shell = current.contains("$(") || current.contains('|') || current.contains('"');
        out.push(Invocation {
            command: command.to_string(),
            flags: current
                .split_whitespace()
                // Strip synopsis brackets *before* filtering, or `[--status` never looks like a
                // flag and every optional flag in the docs goes unchecked.
                .map(|w| w.trim_matches(|c| c == '[' || c == ']'))
                .filter(|w| w.starts_with("--"))
                .map(str::to_string)
                .collect(),
            runnable: !notation && !shell,
            line: current,
        });
    }
    out
}

#[test]
fn the_skill_description_names_every_status_it_counts() {
    // The description is what selects the skill, and it makes a numeric claim ("eleven-state").
    // It listed ten for a while, omitting `open`.
    let skill = doc("SKILL.md");
    let front = skill
        .split("---")
        .nth(1)
        .expect("SKILL.md opens with frontmatter");
    let missing: Vec<&str> = TaskStatus::ALL
        .iter()
        .map(|s| s.as_str())
        .filter(|name| !front.contains(*name))
        .collect();
    assert!(
        missing.is_empty(),
        "the frontmatter says eleven-state but omits {missing:?}"
    );
    assert!(
        front.contains("eleven-state"),
        "if the count changes, this test and the description must change together"
    );
}
