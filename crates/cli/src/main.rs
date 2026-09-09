//! taskforge CLI.
//!
//! Two output modes. With `--json` every command prints the documented envelope, so an agent can
//! branch on the exit status and read `errors[0].code` without parsing prose. Without it, output
//! is human-readable text and errors go to stderr — that form is for people and is not a
//! contract.

use clap::{Parser, Subcommand};
use serde::Serialize;
use std::path::PathBuf;
use taskforge_core::fsstore::FsStore;
use taskforge_core::hooks::{HookConfig, HookEngine};
use taskforge_core::model::{
    parse_age, DueStrategy, Owner, OwnerType, Recurrence, RecurrenceFrequency, Task, TaskKind,
    TaskStatus,
};
use taskforge_core::service::{TaskError, TaskService};
use taskforge_core::store::{AuditEntry, TaskStore};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Provenance baked in by `build.rs`. See that file for why the commit is not optional decoration.
const GIT_COMMIT: &str = env!("TASKFORGE_GIT_COMMIT");
const SOURCE_DIR: &str = env!("TASKFORGE_SOURCE_DIR");
const BUILD_EPOCH: &str = env!("TASKFORGE_BUILD_EPOCH");

/// What `--version` prints: `0.3.0 (60610a0)`.
///
/// The envelope's `taskforge_version` deliberately stays the bare semver — it is a published
/// contract and an agent comparing it against a number should not have to strip a suffix. The
/// commit belongs where a human is looking.
const VERSION_WITH_COMMIT: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("TASKFORGE_GIT_COMMIT"),
    ")"
);

/// Whether to print the JSON envelope. Set once from the parsed flags before anything is
/// emitted; a global because `emit()` is reached from dozens of places and threading a flag
/// through every one of them would obscure the code paths it is meant to decorate.
static JSON_OUTPUT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Serialize)]
struct Envelope {
    ok: bool,
    command: String,
    taskforge_version: String,
    data: Option<serde_json::Value>,
    warnings: Vec<Message>,
    errors: Vec<Message>,
}

/// One entry of `warnings` or `errors`.
///
/// `code` and `message` are the contract every entry carries. `detail` is optional structured
/// context — a hook's id, exit code, and timeout flag — so an agent can branch on the specifics
/// without parsing the message prose.
#[derive(Serialize)]
struct Message {
    code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<serde_json::Value>,
}

impl Message {
    fn new(code: &str, message: String) -> Self {
        Message {
            code: code.into(),
            message,
            detail: None,
        }
    }

    fn with_detail(code: &str, message: String, detail: serde_json::Value) -> Self {
        Message {
            code: code.into(),
            message,
            detail: Some(detail),
        }
    }
}

impl Envelope {
    fn ok(command: &str, data: serde_json::Value) -> Self {
        Envelope {
            ok: true,
            command: command.into(),
            taskforge_version: VERSION.into(),
            data: Some(data),
            warnings: Vec::new(),
            errors: Vec::new(),
        }
    }

    fn err(command: &str, code: &str, message: String) -> Self {
        Envelope {
            ok: false,
            command: command.into(),
            taskforge_version: VERSION.into(),
            data: None,
            warnings: Vec::new(),
            errors: vec![Message::new(code, message)],
        }
    }

    /// Attach post-commit warnings to a successful response.
    fn warn(mut self, warnings: Vec<Message>) -> Self {
        self.warnings = warnings;
        self
    }

    /// Print and exit. `--json` prints the envelope; otherwise a human-readable rendering,
    /// with failures on stderr so stdout stays clean whichever mode is in use.
    fn emit(self) -> ! {
        let code = if self.ok { 0 } else { 1 };
        if JSON_OUTPUT.load(std::sync::atomic::Ordering::Relaxed) {
            println!(
                "{}",
                serde_json::to_string_pretty(&self).unwrap_or_default()
            );
        } else if self.ok {
            print!("{}", render(&self.command, self.data.as_ref()));
        } else {
            for e in &self.errors {
                eprintln!("error: {}: {}", e.code, e.message);
            }
        }
        std::process::exit(code)
    }
}

/// Human-readable rendering of a successful response.
///
/// Lists become a board-shaped table; a single task becomes a field block; anything else is a
/// one-line acknowledgement. Deliberately not a general JSON pretty-printer: the point is that
/// the common cases read at a glance.
fn render(command: &str, data: Option<&serde_json::Value>) -> String {
    let Some(data) = data else {
        return format!("{command}: ok\n");
    };

    if let Some(rows) = data.as_array() {
        if rows.is_empty() {
            return "no tasks\n".to_string();
        }
        // A list of tasks renders as the board; any other array (owners, workspaces) as names.
        if rows[0].get("status").is_none() {
            let names: Vec<String> = rows.iter().map(summary_line).collect();
            return names.join("\n") + "\n";
        }
        return task_table(rows);
    }

    if data.get("status").is_some() && data.get("id").is_some() {
        // A mutation returns the whole task; a human wants one line for that, and the full
        // block only when they asked to see the task.
        if command == "task show" {
            return task_detail(data);
        }
        return format!(
            "{}  {}  {}\n",
            str_of(data, "id"),
            str_of(data, "status"),
            str_of(data, "title")
        );
    }

    format!("{}: {}\n", command, summary_line(data))
}

/// Refuse an owner that is not registered, naming the ones that are.
///
/// Listing them matters more than it looks: `SKILL.md` is the always-loaded file and
/// `reference.md` is explicitly not loaded until needed, so an agent whose first action is
/// `task create` can hit this with no owner-registry guidance in its context at all. Putting the
/// valid names in the message means the fix arrives where the caller already is — the same
/// reasoning as `kind_message` below.
fn require_owner(label: &str, root: &std::path::Path, name: &str) {
    let owners = load_owners(root);
    if owners.iter().any(|o| o.name == name) {
        return;
    }
    let known: Vec<&str> = owners.iter().map(|o| o.name.as_str()).collect();
    // An empty registry is the likeliest moment to hit this, and "registered owners are:" with
    // nothing after it is worse than useless — name the command that fixes it instead.
    let hint = if known.is_empty() {
        "no owners are registered yet — add one with \
         `taskforge owner add --name <you> --type human`"
            .to_string()
    } else {
        format!("registered owners are: {}", known.join(", "))
    };
    Envelope::err(
        label,
        "OWNER_NOT_FOUND",
        format!("owner not in registry: {name}; {hint}"),
    )
    .emit()
}

/// Refuse a date no reader could parse, before anything is written.
///
/// The write side and `model::parse_date` are deliberately the *same* predicate. When they were
/// two, `--expected-by 2026-09-08` was accepted and stored, and then `--overdue` — which parsed
/// strict RFC3339 — matched nothing, because an unreadable date cannot be told apart from a task
/// that is not late. That failure is silent by construction, so it has to be caught here.
fn require_date(label: &str, flag: &str, value: Option<&String>) {
    let Some(v) = value else { return };
    if taskforge_core::model::parse_date(v).is_some() {
        return;
    }
    Envelope::err(
        label,
        "INVALID_DATE",
        format!(
            "cannot read {v:?} as a date for {flag}; expected YYYY-MM-DD (e.g. 2026-09-08) or a \
             full RFC3339 timestamp (e.g. 2026-09-08T17:00:00Z)"
        ),
    )
    .emit()
}

/// An unknown-kind message that names the whole legal set, so the fix is in the error.
fn kind_message(e: &taskforge_core::model::UnknownKind) -> String {
    let legal: Vec<&str> = TaskKind::ALL.iter().map(|k| k.as_str()).collect();
    format!("{e}; expected one of: {}", legal.join(", "))
}

/// The REVIEW column for one row. A task can carry several reviews and the column is narrow, so
/// show the first and count the rest rather than truncating mid-id.
fn reviews_cell(t: &serde_json::Value) -> String {
    let Some(list) = t.get("reviews").and_then(|r| r.as_array()) else {
        return "-".to_string();
    };
    match list.len() {
        0 => "-".to_string(),
        1 => str_of_raw(&list[0]),
        n => format!("{} (+{})", str_of_raw(&list[0]), n - 1),
    }
}

fn str_of(v: &serde_json::Value, key: &str) -> String {
    match v.get(key) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Null) | None => "-".to_string(),
        Some(other) => other.to_string(),
    }
}

/// Best-effort one-line label for a non-task object: its name, else its id, else compact JSON.
fn summary_line(v: &serde_json::Value) -> String {
    for key in ["name", "id"] {
        if let Some(serde_json::Value::String(s)) = v.get(key) {
            return s.clone();
        }
    }
    v.to_string()
}

/// The orchestrate board columns, in board order.
fn task_table(rows: &[serde_json::Value]) -> String {
    let mut out = format!(
        "{:<10} {:<19} {:<22} {:<14} {:<21} {}\n",
        "ID", "STATUS", "WORKER", "REVIEW", "EXPECTED-BY", "TITLE"
    );
    for t in rows {
        let mut title = str_of(t, "title");
        // Dependencies are why a row is not moving, so they belong in the glance.
        if let Some(b) = t.get("blocked_by").and_then(|b| b.as_array()) {
            if !b.is_empty() {
                let ids: Vec<String> = b
                    .iter()
                    .map(|x| x.as_str().unwrap_or("?").to_string())
                    .collect();
                title = format!("{title}  [blocked by {}]", ids.join(", "));
            }
        }
        out.push_str(&format!(
            "{:<10} {:<19} {:<22} {:<14} {:<21} {}\n",
            str_of(t, "id"),
            str_of(t, "status"),
            str_of(t, "worker"),
            reviews_cell(t),
            str_of(t, "expected_by"),
            title
        ));
    }
    out
}

/// Field block for one task. Empty optional fields are omitted rather than printed as dashes,
/// so what is shown is what is set.
fn task_detail(t: &serde_json::Value) -> String {
    let mut out = format!("{}  {}\n", str_of(t, "id"), str_of(t, "title"));
    let always = [
        ("status", "status"),
        ("owner", "owner"),
        ("workspace", "workspace"),
    ];
    for (label, key) in always {
        out.push_str(&format!("  {label:<16} {}\n", str_of(t, key)));
    }
    let optional = [
        ("kind", "kind"),
        ("ticket", "ticket"),
        ("review required", "review_required"),
        ("reviewer", "reviewer"),
        ("expected by", "expected_by"),
        ("worker", "worker"),
        ("checkout", "checkout"),
        ("due", "due_at"),
        ("completed", "completed_at"),
        ("parent", "parent_task_id"),
        ("prior occurrence", "prior_occurrence_id"),
    ];
    for (label, key) in optional {
        match t.get(key) {
            Some(serde_json::Value::Null) | None => {}
            Some(serde_json::Value::Bool(false)) => {}
            Some(v) => out.push_str(&format!("  {label:<16} {}\n", str_of_raw(v))),
        }
    }
    for (label, key) in [
        ("reviews", "reviews"),
        ("tags", "tags"),
        ("blocked by", "blocked_by"),
        ("attachments", "attachment_refs"),
        ("artifacts", "artifact_refs"),
    ] {
        if let Some(a) = t.get(key).and_then(|x| x.as_array()) {
            if !a.is_empty() {
                let items: Vec<String> = a.iter().map(str_of_raw).collect();
                out.push_str(&format!("  {label:<16} {}\n", items.join(", ")));
            }
        }
    }
    out.push_str(&format!("  {:<16} {}\n", "version", str_of(t, "version")));
    if let Some(serde_json::Value::String(d)) = t.get("description") {
        if !d.is_empty() {
            out.push_str(&format!("\n{d}\n"));
        }
    }
    out
}

fn str_of_raw(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[derive(Parser)]
#[command(name = "taskforge", version = VERSION_WITH_COMMIT, about = "Local task management for one human and many agents")]
struct Cli {
    /// Repository root. Defaults to $TASKFORGE_ROOT, else ~/.taskforge.
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    /// Workspace to operate in.
    #[arg(long, global = true, default_value = "main")]
    workspace: String,
    /// Emit the JSON envelope instead of human-readable text. Agents should always pass it.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Top,
}

#[derive(Subcommand)]
enum Top {
    /// Create the repository skeleton.
    Init,
    /// Manage the owner registry.
    Owner {
        #[command(subcommand)]
        command: OwnerCmd,
    },
    /// Create and drive tasks.
    Task {
        #[command(subcommand)]
        command: TaskCmd,
    },
    /// Manage workspaces.
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCmd,
    },
    /// Report whether this binary is built from the current source, and how to fix it if not.
    Doctor,
}

/// How the running binary relates to the checkout it was built from.
#[derive(Debug, PartialEq, Eq)]
enum Freshness {
    Fresh,
    /// `behind` is `None` when the distance cannot be counted — see the tests.
    Stale {
        behind: Option<u32>,
    },
    /// Nothing can be concluded, and saying so is the point. The alternative is calling an
    /// unknown state "fresh", which is the reassuring answer and the wrong one.
    Unknown(&'static str),
}

/// Compare the commit baked into this binary against the checkout's current HEAD.
///
/// Pure, and separated from the `git` calls that feed it, because the interesting case — a stale
/// binary — cannot be produced by a test that runs the binary it just built. The IO shell is
/// `doctor` below; everything worth asserting is here.
fn freshness(baked: &str, source_head: Option<&str>, behind: Option<u32>) -> Freshness {
    if baked == "unknown" {
        return Freshness::Unknown("this binary was built outside a git checkout");
    }
    match source_head {
        None => Freshness::Unknown("the source checkout is missing or not a git repository"),
        Some(head) if head == baked => Freshness::Fresh,
        Some(_) => Freshness::Stale { behind },
    }
}

/// Exit status for a verdict. Only staleness fails, so `doctor` can gate a script; `Unknown` is
/// not a failure because "cannot tell" is not the same as "wrong", and a release build with no
/// `.git` would otherwise fail forever.
fn exit_code(f: &Freshness) -> i32 {
    match f {
        Freshness::Stale { .. } => 1,
        Freshness::Fresh | Freshness::Unknown(_) => 0,
    }
}

#[derive(Subcommand)]
enum WorkspaceCmd {
    Add {
        #[arg(long)]
        name: String,
    },
    List,
}

#[derive(Subcommand)]
enum OwnerCmd {
    Add {
        #[arg(long)]
        name: String,
        #[arg(long = "type", value_parser = ["human", "agent"])]
        owner_type: String,
    },
    List,
}

#[derive(Subcommand)]
enum TaskCmd {
    Create {
        #[arg(long)]
        title: String,
        #[arg(long)]
        owner: String,
        #[arg(long)]
        actor: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        review_required: bool,
        /// Parent task, making this a subtask. One level of nesting only.
        #[arg(long)]
        parent: Option<String>,
        /// What sort of work this is, for reporting. One of the closed set.
        #[arg(long)]
        kind: Option<String>,
        /// External tracker item this task delivers. An opaque string.
        #[arg(long)]
        ticket: Option<String>,
        /// Open-ended label. Repeat for several.
        #[arg(long = "tag")]
        tags: Vec<String>,
    },
    Show {
        #[arg(long)]
        id: String,
    },
    List {
        #[arg(long)]
        status: Option<String>,
        /// Include archived and soft-deleted tasks, which are hidden by default.
        #[arg(long)]
        archived: bool,
        /// Only tasks whose `expected_by` has passed. Finished tasks never qualify.
        #[arg(long)]
        overdue: bool,
        /// Only tasks untouched for longer than this — `12h`, `7d`, `2w`, or a bare number of
        /// days. Finished tasks never qualify; `merged` does, because it waits on a human.
        #[arg(long)]
        stale: Option<String>,
    },
    /// open|pending -> running
    Start(Act),
    /// running -> in-review
    RequestReview(Act),
    /// in-review -> merged
    Merge(Act),
    /// merged -> done (human acceptance; the only success terminal)
    Accept(Act),
    /// running -> done, for work that needs no review
    Complete(Act),
    /// in-review -> changes-requested
    Reject(Act),
    /// -> pending: planned into a wave, not yet dispatched
    Pending(Act),
    /// -> waiting-on-schedule: in flight on a slow non-review step, still on schedule
    Wait(Act),
    /// -> stuck: needs a decision, or a wait has passed its expected-by
    Block(Act),
    /// -> failed: attempted and failed. May be retried with `start`
    Fail(Act),
    /// -> cancelled: will never run, e.g. a dependency failed permanently
    Cancel(Act),
    /// Move to any status by name. Guarded by the same transition table as the named
    /// commands, so this is a shorthand rather than a way around them.
    SetStatus {
        #[command(flatten)]
        act: Act,
        #[arg(long)]
        status: String,
    },
    /// Record which review gates this task and when the wait becomes overdue.
    SetReview {
        #[command(flatten)]
        act: Act,
        #[arg(long)]
        review_id: Option<String>,
        #[arg(long)]
        expected_by: Option<String>,
    },
    /// Record who holds this task and which development checkout they hold it in.
    ///
    /// `--checkout` is the dev workspace (a numbered dev workspace, a worktree name), which is
    /// a different thing from the global `--workspace` flag that selects a taskforge partition.
    SetWorker {
        #[command(flatten)]
        act: Act,
        #[arg(long)]
        worker: Option<String>,
        #[arg(long)]
        checkout: Option<String>,
    },
    AddBlocker {
        #[command(flatten)]
        act: Act,
        #[arg(long)]
        blocked_by: String,
    },
    AddWorklog(Note),
    AddComment(Note),
    SetTitle {
        #[command(flatten)]
        act: Act,
        #[arg(long)]
        title: String,
    },
    SetDescription {
        #[command(flatten)]
        act: Act,
        #[arg(long)]
        description: String,
    },
    Assign {
        #[command(flatten)]
        act: Act,
        #[arg(long)]
        owner: String,
    },
    SetReviewer {
        #[command(flatten)]
        act: Act,
        #[arg(long)]
        reviewer: String,
    },
    SetDue {
        #[command(flatten)]
        act: Act,
        #[arg(long)]
        due_at: String,
    },
    SetReviewRequired {
        #[command(flatten)]
        act: Act,
        /// `ArgAction::Set` so this reads `--value true`, matching the documented surface. A
        /// bare `bool` would make it a flag and reject the explicit value.
        #[arg(long, action = clap::ArgAction::Set)]
        value: bool,
    },
    RemoveBlocker {
        #[command(flatten)]
        act: Act,
        #[arg(long)]
        blocked_by: String,
    },
    /// Classify the work, for reporting.
    SetKind {
        #[command(flatten)]
        act: Act,
        #[arg(long)]
        kind: String,
    },
    /// Record the external tracker item this task delivers.
    SetTicket {
        #[command(flatten)]
        act: Act,
        #[arg(long)]
        ticket: String,
    },
    AddTag {
        #[command(flatten)]
        act: Act,
        #[arg(long)]
        tag: String,
    },
    RemoveTag {
        #[command(flatten)]
        act: Act,
        #[arg(long)]
        tag: String,
    },
    /// Append a review to the task's list, for work spanning several packages.
    AddReview {
        #[command(flatten)]
        act: Act,
        #[arg(long)]
        review_id: String,
    },
    RemoveReview {
        #[command(flatten)]
        act: Act,
        #[arg(long)]
        review_id: String,
    },
    /// Recurrence schedule for a task.
    SetRecurrence {
        #[command(flatten)]
        act: Act,
        #[arg(long, value_parser = ["hourly", "daily", "weekly", "monthly"])]
        frequency: String,
        #[arg(long, default_value_t = 1)]
        interval: u32,
        #[arg(long, value_parser = ["none", "relative", "absolute"], default_value = "none")]
        due_strategy: String,
    },
    ClearRecurrence {
        #[command(flatten)]
        act: Act,
    },
    Tree {
        #[arg(long)]
        id: String,
    },
    Search {
        #[arg(long)]
        text: String,
    },
    Audit {
        #[arg(long)]
        id: String,
    },
    AddAttachment(FileRef),
    AddArtifact(FileRef),
    Archive {
        #[command(flatten)]
        act: Act,
    },
    SoftDelete {
        #[command(flatten)]
        act: Act,
    },
}

#[derive(clap::Args)]
struct FileRef {
    #[arg(long)]
    id: String,
    #[arg(long)]
    path: PathBuf,
    /// `copy` duplicates the file into the task folder; `link` records the path only.
    #[arg(long, value_parser = ["copy", "link"], default_value = "copy")]
    mode: String,
    #[arg(long)]
    actor: String,
    /// Expected current version, for optimistic concurrency.
    #[arg(long)]
    version: Option<i64>,
}

#[derive(clap::Args)]
struct Act {
    #[arg(long)]
    id: String,
    #[arg(long)]
    actor: String,
    /// Expected current version, for optimistic concurrency.
    #[arg(long)]
    version: Option<i64>,
}

#[derive(clap::Args)]
struct Note {
    #[arg(long)]
    id: String,
    #[arg(long)]
    actor: String,
    #[arg(long)]
    text: String,
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

fn resolve_root(explicit: Option<PathBuf>) -> PathBuf {
    // TASKFORGE_ROOT exists so tests (and parallel experiments) never touch the real store.
    explicit
        .or_else(|| std::env::var_os("TASKFORGE_ROOT").map(PathBuf::from))
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default();
            home.join(".taskforge")
        })
}

/// Hooks declared in `config.json`. A malformed or absent config yields no hooks rather than
/// an error: task management must keep working when the notification config is broken.
fn load_hooks(root: &std::path::Path) -> Vec<HookConfig> {
    std::fs::read_to_string(root.join("config.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| serde_json::from_value(v.get("hooks")?.clone()).ok())
        .unwrap_or_default()
}

fn owners_path(root: &std::path::Path) -> PathBuf {
    root.join("owners.json")
}

fn load_owners(root: &std::path::Path) -> Vec<Owner> {
    std::fs::read_to_string(owners_path(root))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn main() {
    let cli = Cli::parse();
    JSON_OUTPUT.store(cli.json, std::sync::atomic::Ordering::Relaxed);
    let root = resolve_root(cli.root.clone());
    let store = FsStore::new(&root, &cli.workspace);

    match cli.command {
        Top::Init => {
            let label = "init";
            match store.init() {
                Ok(()) => {
                    let _ = std::fs::write(
                        root.join("config.json"),
                        "{\"default_workspace\":\"main\",\"hooks\":[]}\n",
                    );
                    if !owners_path(&root).exists() {
                        let _ = std::fs::write(owners_path(&root), "[]\n");
                    }
                    Envelope::ok(
                        label,
                        serde_json::json!({"root": root, "workspace": cli.workspace}),
                    )
                    .emit()
                }
                Err(e) => Envelope::err(label, "IO_ERROR", e.to_string()).emit(),
            }
        }

        Top::Owner { command } => match command {
            OwnerCmd::Add { name, owner_type } => {
                let label = "owner add";
                let mut owners = load_owners(&root);
                if owners.iter().any(|o| o.name == name) {
                    Envelope::err(
                        label,
                        "OWNER_EXISTS",
                        format!("owner already exists: {name}"),
                    )
                    .emit();
                }
                owners.push(Owner {
                    name: name.clone(),
                    owner_type: if owner_type == "human" {
                        OwnerType::Human
                    } else {
                        OwnerType::Agent
                    },
                    description: None,
                    active: true,
                });
                match serde_json::to_string_pretty(&owners)
                    .map_err(|e| e.to_string())
                    .and_then(|s| {
                        std::fs::write(owners_path(&root), s + "\n").map_err(|e| e.to_string())
                    }) {
                    Ok(()) => Envelope::ok(label, serde_json::json!({"name": name})).emit(),
                    Err(e) => Envelope::err(label, "IO_ERROR", e).emit(),
                }
            }
            OwnerCmd::List => {
                let owners = load_owners(&root);
                Envelope::ok(
                    "owner list",
                    serde_json::to_value(owners).unwrap_or_default(),
                )
                .emit()
            }
        },

        Top::Task { command } => run_task(command, store, &root, &cli.workspace),

        Top::Workspace { command } => match command {
            WorkspaceCmd::Add { name } => {
                let label = "workspace add";
                let ws = FsStore::new(&root, &name);
                match ws.init() {
                    Ok(()) => Envelope::ok(label, serde_json::json!({"name": name})).emit(),
                    Err(e) => Envelope::err(label, "IO_ERROR", e.to_string()).emit(),
                }
            }
            WorkspaceCmd::List => {
                let names: Vec<serde_json::Value> = FsStore::workspaces(&root)
                    .into_iter()
                    .map(|n| serde_json::json!({"name": n}))
                    .collect();
                Envelope::ok("workspace list", serde_json::json!(names)).emit()
            }
        },
        Top::Doctor => doctor(),
    }
}

/// Report whether the running binary matches its source checkout.
///
/// The IO shell around `freshness`: read the checkout's HEAD, count the distance, and render.
/// Deliberately not run automatically on every command — taskforge is called in agent loops, and
/// a `git` subprocess per invocation is a real cost to pay for a check that changes daily at most.
fn doctor() -> ! {
    let label = "doctor";
    let source_head = git_in(SOURCE_DIR, &["rev-parse", "--short=7", "HEAD"]);
    // Counting fails when the baked commit is not an ancestor of HEAD — after a rebase, or a
    // checkout of an unrelated branch. That is still stale, just of unknown distance.
    let behind = git_in(
        SOURCE_DIR,
        &["rev-list", "--count", &format!("{GIT_COMMIT}..HEAD")],
    )
    .and_then(|s| s.parse::<u32>().ok());
    let verdict = freshness(GIT_COMMIT, source_head.as_deref(), behind);

    let (state, note) = match &verdict {
        Freshness::Fresh => (
            "fresh",
            "this binary is built from the current source".to_string(),
        ),
        Freshness::Stale { behind } => (
            "stale",
            match behind {
                Some(n) => format!(
                    "the source is {n} commit{} ahead — reinstall with \
                     `cargo install --locked --path crates/cli`",
                    if *n == 1 { "" } else { "s" }
                ),
                None => "the source has moved and this build is not on it — reinstall with \
                         `cargo install --locked --path crates/cli`"
                    .to_string(),
            },
        ),
        Freshness::Unknown(why) => ("unknown", (*why).to_string()),
    };

    let data = serde_json::json!({
        "state": state,
        "note": note,
        "binary": {
            "version": VERSION,
            "commit": GIT_COMMIT,
            "built_at": build_time(),
        },
        "source": {
            "dir": SOURCE_DIR,
            "commit": source_head,
            "version": source_version(),
        },
        "behind": behind,
    });

    // Not the usual `Envelope::ok`/`err` split: staleness is a true report, not a failed command,
    // so `ok` stays true while the exit status still carries the verdict for a gate to read.
    let env = Envelope::ok(label, data);
    if JSON_OUTPUT.load(std::sync::atomic::Ordering::Relaxed) {
        println!("{}", serde_json::to_string_pretty(&env).unwrap_or_default());
    } else {
        println!(
            "  binary   {VERSION} ({GIT_COMMIT})  built {}",
            build_time()
        );
        println!(
            "  source   {}  ({})",
            source_version().unwrap_or_else(|| "?".into()),
            source_head.as_deref().unwrap_or("?")
        );
        println!("  {}: {note}", state.to_uppercase());
    }
    std::process::exit(exit_code(&verdict))
}

/// Run git inside a directory, or `None` if git is absent, the directory is gone, or it failed.
fn git_in(dir: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// The build timestamp, formatted from the epoch seconds `build.rs` baked in.
fn build_time() -> String {
    BUILD_EPOCH
        .parse::<i64>()
        .ok()
        .and_then(|s| time::OffsetDateTime::from_unix_timestamp(s).ok())
        .and_then(|t| {
            t.format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
        .unwrap_or_else(|| "unknown".into())
}

/// The version currently declared in the source workspace manifest.
///
/// A line scan rather than a TOML parse: one field this project owns is not worth a dependency,
/// and a wrong answer here is cosmetic — the commit comparison is what decides the verdict.
fn source_version() -> Option<String> {
    let manifest =
        std::fs::read_to_string(std::path::Path::new(SOURCE_DIR).join("Cargo.toml")).ok()?;
    let mut in_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[workspace.package]";
            continue;
        }
        if in_package {
            if let Some(v) = line.strip_prefix("version") {
                return Some(
                    v.trim_start_matches([' ', '='])
                        .trim()
                        .trim_matches('"')
                        .to_string(),
                );
            }
        }
    }
    None
}

/// Turn a domain error into the envelope, preserving its stable code.
fn fail(label: &str, e: TaskError) -> ! {
    Envelope::err(label, e.code(), e.to_string()).emit()
}

fn run_task(command: TaskCmd, mut store: FsStore, root: &std::path::Path, workspace: &str) {
    match command {
        TaskCmd::Create {
            title,
            owner,
            actor,
            description,
            review_required,
            parent,
            kind,
            ticket,
            tags,
        } => {
            let label = "task create";
            // Parsed before anything is allocated, so a bad kind creates no task.
            let kind = match kind.as_deref().map(TaskKind::parse) {
                Some(Ok(k)) => Some(k),
                Some(Err(e)) => Envelope::err(label, "INVALID_KIND", kind_message(&e)).emit(),
                None => None,
            };
            require_owner(label, root, &owner);
            // Validate the parent before minting an id, so a rejected create consumes nothing.
            if let Some(p) = &parent {
                match store.get(p) {
                    None => fail(label, TaskError::NotFound(p.clone())),
                    // One level of nesting only. `task tree` renders direct children alone, so a
                    // grandchild would be invisible from its own root — enforced here rather than
                    // left to a comment.
                    Some(t) if t.parent_task_id.is_some() => Envelope::err(
                        label,
                        "INVALID_PARENT",
                        format!("{p} is already a subtask; one level of nesting only"),
                    )
                    .emit(),
                    Some(_) => {}
                }
            }
            let id = match store.next_id() {
                Ok(id) => id,
                Err(e) => Envelope::err(label, "IO_ERROR", e.to_string()).emit(),
            };
            let mut task = Task::new(&id, &title, workspace, &owner, now());
            task.description = description;
            task.review_required = review_required;
            task.parent_task_id = parent;
            task.kind = kind;
            task.ticket = ticket;
            task.tags = tags;
            if let Err(e) = store.put(task.clone()) {
                fail(
                    label,
                    TaskError::Io {
                        id: id.clone(),
                        source: e,
                    },
                );
            }
            let warnings = audit_warning(&mut store, &id, &actor, "created", None, None);
            Envelope::ok(label, serde_json::to_value(task).unwrap_or_default())
                .warn(warnings)
                .emit()
        }

        TaskCmd::Show { id } => {
            let label = "task show";
            match store.get(&id) {
                Some(t) => Envelope::ok(label, serde_json::to_value(t).unwrap_or_default()).emit(),
                None => fail(label, TaskError::NotFound(id)),
            }
        }

        TaskCmd::List {
            status,
            archived,
            overdue,
            stale,
        } => {
            let label = "task list";
            let mut tasks: Vec<Task> = store.list();
            if !archived {
                tasks.retain(|t| !t.archived && !t.soft_deleted);
            }
            if let Some(want) = status {
                match TaskStatus::parse(&want) {
                    Ok(s) => tasks.retain(|t| t.status == s),
                    Err(e) => Envelope::err(label, "INVALID_STATUS", e.to_string()).emit(),
                }
            }
            // Filters compose: each narrows what the previous one left.
            let now = now();
            if overdue {
                tasks.retain(|t| t.is_overdue(&now));
            }
            if let Some(age) = stale {
                match parse_age(&age) {
                    Some(max) => tasks.retain(|t| t.is_stale(&now, max)),
                    None => Envelope::err(
                        label,
                        "INVALID_AGE",
                        format!(
                            "cannot read {age:?} as an age; try 12h, 7d, 2w, or a number of days"
                        ),
                    )
                    .emit(),
                }
            }
            tasks.sort_by(|a, b| a.id.cmp(&b.id));
            Envelope::ok(label, serde_json::to_value(tasks).unwrap_or_default()).emit()
        }

        TaskCmd::Start(a) => transition("task start", store, root, a, TaskStatus::Running),
        TaskCmd::RequestReview(a) => {
            transition("task request-review", store, root, a, TaskStatus::InReview)
        }
        TaskCmd::Merge(a) => transition("task merge", store, root, a, TaskStatus::Merged),
        TaskCmd::Accept(a) => transition("task accept", store, root, a, TaskStatus::Done),
        TaskCmd::Complete(a) => transition("task complete", store, root, a, TaskStatus::Done),
        TaskCmd::Reject(a) => {
            transition("task reject", store, root, a, TaskStatus::ChangesRequested)
        }
        TaskCmd::Pending(a) => transition("task pending", store, root, a, TaskStatus::Pending),
        TaskCmd::Wait(a) => transition("task wait", store, root, a, TaskStatus::WaitingOnSchedule),
        TaskCmd::Block(a) => transition("task block", store, root, a, TaskStatus::Stuck),
        TaskCmd::Fail(a) => transition("task fail", store, root, a, TaskStatus::Failed),
        TaskCmd::Cancel(a) => transition("task cancel", store, root, a, TaskStatus::Cancelled),
        TaskCmd::SetStatus { act, status } => {
            let label = "task set-status";
            match TaskStatus::parse(&status) {
                Ok(to) => transition(label, store, root, act, to),
                Err(e) => Envelope::err(label, "INVALID_STATUS", e.to_string()).emit(),
            }
        }

        TaskCmd::SetReview {
            act,
            review_id,
            expected_by,
        } => {
            // Checked before anything is written, so a bad date does not land a half-applied
            // change that replaced the review id and skipped the window.
            require_date("task set-review", "--expected-by", expected_by.as_ref());
            // An absent flag leaves its own field alone, so one can be set without clearing
            // the other.
            patch("task set-review", &mut store, &act, "set_review", |t| {
                // Replaces the list rather than appending — `add-review` is the append.
                if let Some(id) = review_id {
                    t.reviews = vec![id];
                }
                if expected_by.is_some() {
                    t.expected_by = expected_by;
                }
            })
        }

        TaskCmd::SetWorker {
            act,
            worker,
            checkout,
        } => patch("task set-worker", &mut store, &act, "set_worker", |t| {
            if worker.is_some() {
                t.worker = worker;
            }
            if checkout.is_some() {
                t.checkout = checkout;
            }
        }),

        TaskCmd::AddBlocker { act, blocked_by } => {
            let label = "task add-blocker";
            if store.get(&blocked_by).is_none() {
                fail(label, TaskError::NotFound(blocked_by));
            }
            patch(label, &mut store, &act, "add_blocker", |t| {
                if !t.blocked_by.contains(&blocked_by) {
                    t.blocked_by.push(blocked_by);
                }
            })
        }

        TaskCmd::AddWorklog(n) => {
            let label = "task add-worklog";
            if store.get(&n.id).is_none() {
                fail(label, TaskError::NotFound(n.id));
            }
            match store.append_worklog(&n.id, &n.actor, &now(), &n.text) {
                Ok(()) => Envelope::ok(label, serde_json::json!({"id": n.id})).emit(),
                Err(e) => Envelope::err(label, "IO_ERROR", e.to_string()).emit(),
            }
        }

        TaskCmd::AddComment(n) => {
            let label = "task add-comment";
            if store.get(&n.id).is_none() {
                fail(label, TaskError::NotFound(n.id));
            }
            match store.append_comment(&n.id, &n.actor, &now(), &n.text) {
                Ok(()) => Envelope::ok(label, serde_json::json!({"id": n.id})).emit(),
                Err(e) => Envelope::err(label, "IO_ERROR", e.to_string()).emit(),
            }
        }

        TaskCmd::SetTitle { act, title } => {
            patch("task set-title", &mut store, &act, "set_title", |t| {
                t.title = title
            })
        }
        TaskCmd::SetDescription { act, description } => patch(
            "task set-description",
            &mut store,
            &act,
            "set_description",
            |t| t.description = Some(description),
        ),
        TaskCmd::Assign { act, owner } => {
            let label = "task assign";
            require_owner(label, root, &owner);
            patch(label, &mut store, &act, "assign", |t| t.owner = owner)
        }
        TaskCmd::SetReviewer { act, reviewer } => {
            let label = "task set-reviewer";
            require_owner(label, root, &reviewer);
            patch(label, &mut store, &act, "set_reviewer", |t| {
                t.reviewer = Some(reviewer)
            })
        }
        TaskCmd::SetDue { act, due_at } => {
            require_date("task set-due", "--due-at", Some(&due_at));
            patch("task set-due", &mut store, &act, "set_due", |t| {
                t.due_at = Some(due_at)
            })
        }
        TaskCmd::SetReviewRequired { act, value } => patch(
            "task set-review-required",
            &mut store,
            &act,
            "set_review_required",
            |t| t.review_required = value,
        ),
        TaskCmd::RemoveBlocker { act, blocked_by } => patch(
            "task remove-blocker",
            &mut store,
            &act,
            "remove_blocker",
            |t| t.blocked_by.retain(|b| b != &blocked_by),
        ),
        TaskCmd::SetKind { act, kind } => {
            let label = "task set-kind";
            match TaskKind::parse(&kind) {
                Ok(k) => patch(label, &mut store, &act, "set_kind", |t| t.kind = Some(k)),
                Err(e) => Envelope::err(label, "INVALID_KIND", kind_message(&e)).emit(),
            }
        }
        TaskCmd::SetTicket { act, ticket } => {
            patch("task set-ticket", &mut store, &act, "set_ticket", |t| {
                t.ticket = Some(ticket)
            })
        }
        TaskCmd::AddTag { act, tag } => patch("task add-tag", &mut store, &act, "add_tag", |t| {
            if !t.tags.contains(&tag) {
                t.tags.push(tag);
            }
        }),
        TaskCmd::RemoveTag { act, tag } => {
            patch("task remove-tag", &mut store, &act, "remove_tag", |t| {
                t.tags.retain(|x| x != &tag)
            })
        }
        TaskCmd::AddReview { act, review_id } => {
            patch("task add-review", &mut store, &act, "add_review", |t| {
                if !t.reviews.contains(&review_id) {
                    t.reviews.push(review_id);
                }
            })
        }
        TaskCmd::RemoveReview { act, review_id } => patch(
            "task remove-review",
            &mut store,
            &act,
            "remove_review",
            |t| t.reviews.retain(|r| r != &review_id),
        ),
        TaskCmd::SetRecurrence {
            act,
            frequency,
            interval,
            due_strategy,
        } => {
            let freq = match frequency.as_str() {
                "hourly" => RecurrenceFrequency::Hourly,
                "daily" => RecurrenceFrequency::Daily,
                "weekly" => RecurrenceFrequency::Weekly,
                _ => RecurrenceFrequency::Monthly,
            };
            let strat = match due_strategy.as_str() {
                "relative" => DueStrategy::Relative,
                "absolute" => DueStrategy::Absolute,
                _ => DueStrategy::None,
            };
            patch(
                "task set-recurrence",
                &mut store,
                &act,
                "set_recurrence",
                |t| {
                    t.recurrence = Some(Recurrence {
                        frequency: freq,
                        interval,
                        preserve_review_required: true,
                        carry_forward_description: true,
                        carry_forward_owner: true,
                        carry_forward_due_strategy: strat,
                    })
                },
            )
        }
        TaskCmd::ClearRecurrence { act } => patch(
            "task clear-recurrence",
            &mut store,
            &act,
            "clear_recurrence",
            |t| t.recurrence = None,
        ),
        TaskCmd::Tree { id } => {
            let label = "task tree";
            let Some(root_task) = store.get(&id) else {
                fail(label, TaskError::NotFound(id))
            };
            // One level of nesting only, matching the documented model.
            let kids: Vec<Task> = store
                .list()
                .into_iter()
                .filter(|t| t.parent_task_id.as_deref() == Some(root_task.id.as_str()))
                .collect();
            let mut data = serde_json::to_value(&root_task).unwrap_or_default();
            if let Some(obj) = data.as_object_mut() {
                obj.insert(
                    "subtasks".into(),
                    serde_json::to_value(kids).unwrap_or_default(),
                );
            }
            Envelope::ok(label, data).emit()
        }
        TaskCmd::Search { text } => {
            let needle = text.to_lowercase();
            let hits: Vec<Task> = store
                .list()
                .into_iter()
                .filter(|t| {
                    t.title.to_lowercase().contains(&needle)
                        || t.description
                            .as_deref()
                            .map(|d| d.to_lowercase().contains(&needle))
                            .unwrap_or(false)
                })
                .collect();
            Envelope::ok(
                "task search",
                serde_json::to_value(hits).unwrap_or_default(),
            )
            .emit()
        }
        TaskCmd::Audit { id } => {
            let label = "task audit";
            if store.get(&id).is_none() {
                fail(label, TaskError::NotFound(id));
            }
            Envelope::ok(
                label,
                serde_json::to_value(store.audit(&id)).unwrap_or_default(),
            )
            .emit()
        }
        TaskCmd::AddAttachment(f) => attach("task add-attachment", &mut store, f, "attachments"),
        TaskCmd::AddArtifact(f) => attach("task add-artifact", &mut store, f, "artifacts"),
        TaskCmd::Archive { act } => {
            let label = "task archive";
            match store.get(&act.id) {
                Some(t) if t.archived => Envelope::err(
                    label,
                    "ALREADY_ARCHIVED",
                    format!("{} is already archived", act.id),
                )
                .emit(),
                Some(_) => patch(label, &mut store, &act, "archive", |t| t.archived = true),
                None => fail(label, TaskError::NotFound(act.id.clone())),
            }
        }
        TaskCmd::SoftDelete { act } => {
            patch("task soft-delete", &mut store, &act, "soft_delete", |t| {
                t.soft_deleted = true
            })
        }
    }
}

fn transition(label: &str, store: FsStore, root: &std::path::Path, a: Act, to: TaskStatus) -> ! {
    let mut svc = TaskService::new(store, now()).with_hooks(HookEngine::new(load_hooks(root)));
    match svc.set_status(&a.id, to, &a.actor, a.version) {
        Ok(t) => {
            // A recurring completion produces a successor; report it so the caller is not
            // left guessing whether one was created.
            let mut data = serde_json::to_value(t).unwrap_or_default();
            if let (Some(obj), Some(next)) = (data.as_object_mut(), svc.last_generated.as_ref()) {
                obj.insert("next_occurrence_id".into(), serde_json::json!(next));
            }
            Envelope::ok(label, data)
                .warn(post_commit_warnings(&svc))
                .emit()
        }
        Err(e) => fail(label, e),
    }
}

/// Everything that went wrong after the mutation committed.
///
/// Hooks are notifications, so a broken one cannot fail the command — but staying silent about
/// it left an agent believing its notification fired when the binary did not even exist.
fn post_commit_warnings<S: TaskStore>(svc: &TaskService<S>) -> Vec<Message> {
    let mut out: Vec<Message> = svc
        .last_warnings
        .iter()
        .map(|w| Message::new(w.code, w.message.clone()))
        .collect();
    out.extend(svc.last_hook_results.iter().filter(|r| !r.ok).map(|r| {
        let why = if !r.started {
            "could not be started (missing or not executable)".to_string()
        } else if r.timed_out {
            "timed out".to_string()
        } else {
            format!("exited {}", r.exit_code)
        };
        Message::with_detail(
            "HOOK_FAILED",
            format!("hook {:?} {why}", r.hook_id),
            serde_json::json!({
                "hook_id": r.hook_id,
                "exit_code": r.exit_code,
                "timed_out": r.timed_out,
                "started": r.started,
            }),
        )
    }));
    out
}

/// Record an attachment or artifact reference on a task.
fn attach(label: &str, store: &mut FsStore, f: FileRef, kind: &str) -> ! {
    if store.get(&f.id).is_none() {
        fail(label, TaskError::NotFound(f.id));
    }
    let code = if kind == "attachments" {
        "ATTACHMENT_NOT_FOUND"
    } else {
        "ARTIFACT_NOT_FOUND"
    };
    let reference = match store.attach(&f.id, kind, &f.path, f.mode == "copy") {
        Ok(r) => r,
        Err(e) => Envelope::err(label, code, e.to_string()).emit(),
    };
    let act = Act {
        id: f.id.clone(),
        actor: f.actor.clone(),
        version: f.version,
    };
    let action = if kind == "attachments" {
        "add_attachment"
    } else {
        "add_artifact"
    };
    patch(label, store, &act, action, |t| {
        if kind == "attachments" {
            t.attachment_refs.push(reference);
        } else {
            t.artifact_refs.push(reference);
        }
    })
}

/// Read, mutate, and write back a task, bumping its version. Used by every patch-style
/// setter, so they all agree on the version/timestamp bookkeeping, the concurrency guard, and
/// the audit entry.
///
/// `action` is what lands in `audit.log`. Every setter records one: `--actor` is required on all
/// of them, and requiring a value only to discard it left "who changed this field" unanswerable.
fn patch(
    label: &str,
    store: &mut FsStore,
    act: &Act,
    action: &str,
    f: impl FnOnce(&mut Task),
) -> ! {
    let Some(mut task) = store.get(&act.id) else {
        fail(label, TaskError::NotFound(act.id.clone()))
    };
    // Same optimistic-concurrency guard the status transitions use, so a patch racing another
    // writer is refused rather than silently clobbering it.
    if let Some(expected) = act.version {
        if expected != task.version {
            fail(
                label,
                TaskError::VersionMismatch {
                    id: act.id.clone(),
                    expected,
                    actual: task.version,
                },
            );
        }
    }
    f(&mut task);
    task.updated_at = now();
    task.version += 1;
    if let Err(e) = store.put(task.clone()) {
        fail(
            label,
            TaskError::Io {
                id: act.id.clone(),
                source: e,
            },
        );
    }
    // After the commit, so a failure here cannot un-commit it: reported as a warning.
    let warnings = audit_warning(store, &act.id, &act.actor, action, None, None);
    Envelope::ok(label, serde_json::to_value(task).unwrap_or_default())
        .warn(warnings)
        .emit()
}

/// Append an audit entry, returning a warning list rather than failing.
///
/// The caller has already committed its change, so a lost audit line is an incomplete trail —
/// worth reporting, never worth claiming the mutation did not happen.
fn audit_warning(
    store: &mut FsStore,
    id: &str,
    actor: &str,
    action: &str,
    from: Option<String>,
    to: Option<String>,
) -> Vec<Message> {
    match store.append_audit(AuditEntry {
        ts: now(),
        actor: actor.to_string(),
        action: action.to_string(),
        task_id: id.to_string(),
        from,
        to,
    }) {
        Ok(()) => Vec::new(),
        Err(e) => vec![Message::new(
            "AUDIT_WRITE_FAILED",
            format!("{id} was changed but the audit entry was not written: {e}"),
        )],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_binary_built_from_the_current_head_is_fresh() {
        assert_eq!(
            freshness("60610a0", Some("60610a0"), None),
            Freshness::Fresh
        );
    }

    #[test]
    fn a_binary_built_from_an_older_commit_is_stale() {
        assert_eq!(
            freshness("42471d8", Some("60610a0"), Some(3)),
            Freshness::Stale { behind: Some(3) }
        );
    }

    #[test]
    fn staleness_is_reported_even_when_the_distance_cannot_be_counted() {
        // A rebase or a force-push leaves the baked commit unreachable from HEAD, so
        // `rev-list --count` fails. Not knowing *how far* behind is no reason to stop
        // reporting *that* it is behind — that silence is the whole bug this detects.
        assert_eq!(
            freshness("deadbee", Some("60610a0"), None),
            Freshness::Stale { behind: None }
        );
    }

    #[test]
    fn a_binary_built_outside_a_git_checkout_reports_unknown_rather_than_fresh() {
        // `cargo install` from a published crate or a tarball has no commit to bake. Calling
        // that "fresh" would be a comforting lie.
        assert!(matches!(
            freshness("unknown", Some("60610a0"), None),
            Freshness::Unknown(_)
        ));
    }

    #[test]
    fn an_unreadable_source_checkout_reports_unknown_rather_than_fresh() {
        // The checkout was moved or deleted after installing. Nothing can be concluded.
        assert!(matches!(
            freshness("60610a0", None, None),
            Freshness::Unknown(_)
        ));
    }

    #[test]
    fn only_staleness_is_a_failure_exit() {
        // `doctor` is meant to be usable as a gate, so the exit status has to carry the verdict.
        assert_eq!(exit_code(&Freshness::Fresh), 0);
        assert_eq!(exit_code(&Freshness::Unknown("x")), 0);
        assert_eq!(exit_code(&Freshness::Stale { behind: Some(1) }), 1);
        assert_eq!(exit_code(&Freshness::Stale { behind: None }), 1);
    }
}
