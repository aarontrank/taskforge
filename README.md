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

`cargo install` refreshes the crates.io index, so it needs network even when every dependency is
already cached — offline or in a sandbox it fails with `Could not resolve host: index.crates.io`.
Add `--locked --offline` there and it builds from the committed lockfile.

Output is human-readable by default; pass `--json` for the machine envelope:

```
$ taskforge task list
ID         STATUS              WORKER                 REVIEW         EXPECTED-BY           TITLE
TASK-0001  in-review           addresscr-CR-301625168 CR-301625168   2026-09-03T17:00:00Z  S1 crew list
TASK-0002  merged              -                      -              -                     S2 cast list
TASK-0003  open                -                      -              -                     S3 shared header  [blocked by TASK-0001]
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

`116` tests cover the model, the full transition matrix (all 121 state pairs), the workflow
guards, filesystem round-trips, hook execution including timeouts, and the CLI end to end —
including a test that every one of the eleven statuses is reachable through the binary, and
tests that drive the write path against an unwritable file so a refused write cannot regress
into being reported as a success.

## Licence

MIT.
