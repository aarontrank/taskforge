# TaskForge

A local, file-backed task tracker for one human and several AI agents. Think of it as a small
Jira that lives on your machine: tasks are markdown files you can read and edit, a CLI drives
them, and the workflow is enforced rather than suggested.

Written in Rust. No runtime, no daemon, no database, no Node.

---

## Why the workflow is the point

Most trackers let anything move to "done". TaskForge encodes the two distinctions that
actually matter when agents are doing the work:

- **`merged` is not `done`.** A merged review still awaits human acceptance.
- **`done` means merged *and* accepted.** It is the only success terminal.

That falls out into enforcement you feel immediately: a blocker sitting in `merged` does **not**
release the task waiting on it, and `task complete` on a review-required task is refused. The
states are those of an agent-orchestration status board, so a task can also say *which* review
gates it (`review_id`), *when* the wait becomes overdue (`expected_by`), *who* holds it
(`worker`), and *which dev checkout* they hold it in (`checkout`, deliberately distinct from
taskforge's own `workspace` partition) — which is what separates "in review, on schedule" from
"stuck" as data rather than as a judgement call. The first two are set with `task set-review`,
the last two with `task set-worker`.

```
open ─→ pending ─→ running ─→ in-review ─→ merged ─→ done
                      │           │                   ▲
                      │           └→ changes-requested┘
                      ├→ waiting-on-schedule
                      └→ stuck / failed
```

`cancelled` is reachable from `open`, `pending`, `stuck`, and `failed` — not from work already in
flight. Abandoning a `running` or `in-review` task is `block` then `cancel`, so dropping something
a reviewer may already be holding takes two decisions.

Eleven states: `open`, `pending`, `running`, `in-review`, `changes-requested`, `merged`,
`waiting-on-schedule`, `stuck`, `done`, `failed`, `cancelled` — each reachable from the CLI,
either by its named command or via `task set-status --status <name>`. `archived` and `soft_deleted` are
orthogonal flags, not statuses, so archiving a task does not erase what state it was in.

---

## Install

Requires a Rust toolchain (1.85+).

```bash
git clone git@github.com:aarontrank/taskforge.git
cd taskforge
cargo install --path crates/cli     # puts `taskforge` on your PATH
```

Re-run that same command after pulling changes; it overwrites in place. To remove the binary
later: `cargo uninstall taskforge-cli`.

### Is the installed binary current?

```bash
taskforge --version      # 0.3.0 (60610a0) — semver plus the commit it was built from
taskforge doctor         # compares that commit against this checkout; exit 1 if stale
```

Worth the two commands, because forgetting the reinstall is not a hypothetical: the installed
binary once sat **eight commits behind** for a week. `kind`, `--stale` and `--overdue` were
written, tested and committed, and none of them had ever run — while `--version` reported `0.2.0`
for both builds, so nothing could say so. `doctor` is the answer to "is this flag missing, or is
my install old?", which is otherwise a question you have to already suspect to ask.

### Releasing

**Bump `version` in the workspace `Cargo.toml` in the same commit as the change.** Pre-1.0: minor
for anything that adds or changes a command or its output, patch for a fix. The number is what
makes a build *nameable* in a bug report.

It is not, however, what makes drift *detectable* — a version only moves when someone remembers
to move it, and forgetting is what caused the incident above. That is why the commit is baked in
at build time (`crates/cli/build.rs`): the hash cannot be forgotten, so `doctor` still gives a
true answer on a release where the bump was missed.

`cargo install` refreshes the crates.io index, so it needs network even when every dependency is
already cached — offline or in a sandbox it fails with `Could not resolve host: index.crates.io`.
Add `--locked --offline` there and it builds from the committed lockfile.

Output is human-readable by default; pass `--json` for the machine envelope:

```
$ taskforge task list
ID         STATUS              WORKER                 REVIEW         EXPECTED-BY           TITLE
TASK-0001  in-review           review-worker-1        PR-4821        2026-09-03T17:00:00Z  Port the storage layer
TASK-0002  merged              -                      -              -                     Port the CLI
TASK-0003  open                -                      -              -                     Shared header  [blocked by TASK-0001]
```

Run the tests and lints:

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```

There is no CI on this repository, so nothing runs those but you. Run them before committing.

---

## Quick start

`init` and at least one `owner add` are **both required**, and every later `--owner`/`--actor` is
validated against that registry — skip them and each `task create` fails with
`OWNER_NOT_FOUND`. Both are idempotent, so re-running them is safe.

```bash
taskforge init --json
taskforge owner add --name aaron --type human --json
taskforge owner add --name agent --type agent --json

taskforge task create --title "Port the storage layer" --owner agent --actor aaron --json
taskforge task start    --id TASK-0001 --actor agent --json
taskforge task complete --id TASK-0001 --actor agent --json
```

Review-gated instead:

```bash
taskforge task create --title "Risky change" --owner agent --actor aaron --review-required --json
taskforge task start          --id TASK-0002 --actor agent --json
taskforge task request-review --id TASK-0002 --actor agent --json
taskforge task merge          --id TASK-0002 --actor agent --json   # merged: not done
taskforge task accept         --id TASK-0002 --actor aaron --json   # done
```

---

## Where data lives

**One location, always: `~/.taskforge`.** It does not vary with your working directory. Set
`TASKFORGE_ROOT` to point somewhere else (tests and experiments use this so they never touch
your real store), or pass `--root <path>` per command.

```
~/.taskforge/
  config.json            # default workspace and hook definitions
  owners.json            # owner registry
  task_counter.json      # id sequence
  workspaces/
    main/
      tasks/
        TASK-0001/
          task.md        # YAML frontmatter, then Summary / Acceptance Criteria / Notes
          worklog.md     # append-only execution log
          comments.md    # append-only discussion
          audit.log      # append-only NDJSON mutation history
          attachments/
          artifacts/
          subtasks/
```

Every task is a folder of plain text. Read it, grep it, back it up with anything.

---

## For agents

Install the Claude Code skill, which teaches an agent the CLI and the workflow rules:

```bash
./skill/install.sh                      # ~/.claude/skills/taskforge
./skill/install.sh --project /some/repo # project-local instead
./skill/uninstall.sh
```

Then `/reload-plugins` in a live session, or start a new one.

The skill's own reference lives in [`skill/taskforge/reference.md`](skill/taskforge/reference.md)
(full command surface and error codes) and
[`skill/taskforge/workflows.md`](skill/taskforge/workflows.md) (worked scenarios). Those are the
authoritative CLI docs; this README deliberately does not duplicate the whole flag list.

The rules that matter for an agent: always `--json`, read before mutating, use patch commands
rather than editing `task.md`, pass `--actor`, log progress with `add-worklog`, and treat
`in-review` inside its `expected_by` window as waiting rather than stuck.

---

## Using it as an orchestration board

This is what the eleven states are for. The `orchestrate` skill coordinates parallel work
streams and keeps a status board of them, and it **prefers taskforge as that board whenever the
binary is on `PATH`**, falling back to a hand-maintained markdown table when it is not. The
status vocabulary here is deliberately that board's vocabulary, so one task is one stream.

The reason to prefer it is that it *enforces* what a markdown table can only describe: a stream
whose blockers are unresolved cannot be dispatched, a review-gated stream cannot be completed
directly, every change lands in an audit log, and `merged` is not `done`. A table can be written
into any state by any worker, correct or not.

Two things to know if you wire it up:

- **The store is chosen once, at setup, and recorded.** Not per operation — a `PATH` difference
  mid-run would switch stores and orphan whatever was already recorded.
- **Waves and the critical path are not stored here.** They stay in the upstream plan artifact,
  so ordering is read from the plan against this board.

`taskforge task list` renders the board columns directly. The operation-by-operation mapping
lives in the `orchestrate` skill rather than here, so there is one copy of it.

---

## Concurrency, audit, and hooks

**Optimistic concurrency.** Pass `--version <n>` with the version you read; a stale value fails
with `CONFLICT_VERSION_MISMATCH` instead of silently overwriting. Available on every mutating
command — status transitions and field setters alike.

**Audit.** Every mutation appends a line to the task's `audit.log`, naming the `--actor` that
caused it and the action: `created`, `status_changed`, `set_review`, `assign`, `archive`, and so
on. Read it with `taskforge task audit --id TASK-0001 --json`.

**Writes are fallible and say so.** A refused write — full disk, read-only mount, bad permissions
— fails the command with `IO_ERROR` and stores nothing. The CLI never reports a status or version
it did not persist.

**Classification, for reporting.** A task carries `kind` (a closed set: `feature`, `bug`, `chore`,
`investigation`, `oncall`, `doc`), open-ended `tags`, and an opaque `ticket` naming the external
tracker item it delivers. `kind` is closed on purpose: free text drifts into `bug`/`bugfix`/`Bug`
and the drift only shows up as a wrong number in a report months later. No kind means
unclassified, which is a reportable answer rather than a guess.

**Reviews are plural.** Work crossing package ownership boundaries needs one review per package,
so `reviews` is a list. Files written when it was singular still load and migrate on write.

**Finding work that stopped moving.** `task list --overdue` catches anything past its
`expected_by`; `task list --stale 7d` catches anything untouched for longer than an age
(`12h`/`7d`/`2w`, or a bare number of days). Both skip the terminal statuses — a task finished in
January has not moved since, and that is correct — and both *include* `merged`, which is the state
that rots while waiting on a human to accept it.

**Hooks.** Local commands fired *after* a mutation commits, configured in `config.json`, with
the event payload on stdin. A hook that fails, hangs, or does not exist never rolls the change
back — hooks are notifications, not gates — and is reported as a `HOOK_FAILED` entry in the
response's `warnings` while the command still exits 0. Details in the skill reference.

**`warnings` on a successful response.** Anything that went wrong after the change committed
lands here rather than failing the command: a hook that never fired, an audit line that could not
be written, a recurring successor that could not be created. `ok: true` means the mutation
happened, not that nothing went wrong.

---

## Recurrence

```bash
taskforge task set-recurrence --id TASK-0001 --frequency weekly --due-strategy absolute --actor aaron --json
```

Completing a recurring task generates its successor and returns `next_occurrence_id`. The
successor links back via `prior_occurrence_id` and inherits the schedule. Month arithmetic
clamps rather than overflowing: 31 January plus one month is 28 February.

---

## Project layout

```
crates/core/    domain model, storage, status machine, hook engine
crates/cli/     the `taskforge` binary
skill/          Claude Code skill + installer
```

`crates/core` is split so the status machine is tested against an in-memory store with no
filesystem, while the filesystem layer is tested against a temp directory.

---

## Status

The Rust implementation is the only implementation; the earlier TypeScript version and its
Next.js web UI have been removed. Human inspection is the markdown files themselves plus
`taskforge task list`.

`177` tests cover the model, the full transition matrix (all 121 state pairs), the workflow
guards, filesystem round-trips, hook execution including timeouts, and the CLI end to end —
including a test that every one of the eleven statuses is reachable through the binary, and
tests that drive the write path against an unwritable file so a refused write cannot regress
into being reported as a success.

Seven of them check the **skill docs** rather than the code (`crates/cli/tests/docs.rs`). An agent
learns this tool by reading `skill/taskforge/`, so a wrong claim there misleads it exactly as a
bug would, and silently. The transition table in `reference.md` is parsed and diffed against
`can_transition`; every documented flag is checked against that command's `--help`; every
runnable example is executed; every error and warning code the CLI emits must appear in the
reference. The rule: no factual claim about the CLI is verified by reading it.

## Possible future work

[`ENHANCEMENTS.md`](ENHANCEMENTS.md) records ideas that were scoped and deliberately deferred —
reports, reflection mining, review reconciliation, a read-only dashboard — each with the reason
it is not built and the observation that would justify building it. Several have a cheap
alternative that works today and is noted alongside.

## Licence

MIT.
