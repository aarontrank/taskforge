//! taskforge CLI.
//!
//! Every command prints the documented JSON envelope and exits non-zero on error, so an agent
//! can branch on the exit status and read `errors[0].code` without parsing prose.

use clap::{Parser, Subcommand};
use serde::Serialize;
use std::path::PathBuf;
use taskforge_core::fsstore::FsStore;
use taskforge_core::hooks::{HookConfig, HookEngine};
use taskforge_core::model::{
    DueStrategy, Owner, OwnerType, Recurrence, RecurrenceFrequency, Task, TaskStatus,
};
use taskforge_core::service::{TaskError, TaskService};
use taskforge_core::store::TaskStore;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Serialize)]
struct Envelope {
    ok: bool,
    command: String,
    taskforge_version: String,
    data: Option<serde_json::Value>,
    warnings: Vec<Message>,
    errors: Vec<Message>,
}

#[derive(Serialize)]
struct Message {
    code: String,
    message: String,
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
            errors: vec![Message {
                code: code.into(),
                message,
            }],
        }
    }

    fn emit(self) -> ! {
        let code = if self.ok { 0 } else { 1 };
        println!(
            "{}",
            serde_json::to_string_pretty(&self).unwrap_or_default()
        );
        std::process::exit(code)
    }
}

#[derive(Parser)]
#[command(name = "taskforge", version = VERSION, about = "Local task management for one human and many agents")]
struct Cli {
    /// Repository root. Defaults to $TASKFORGE_ROOT, else ~/.taskforge.
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    /// Workspace to operate in.
    #[arg(long, global = true, default_value = "main")]
    workspace: String,
    /// Emit JSON. Accepted everywhere and always on, since JSON is the only output form.
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
    /// Record which review gates this task, when the wait is overdue, and who holds it.
    SetReview {
        #[arg(long)]
        id: String,
        #[arg(long)]
        actor: String,
        #[arg(long)]
        review_id: Option<String>,
        #[arg(long)]
        expected_by: Option<String>,
        #[arg(long)]
        worker: Option<String>,
    },
    AddBlocker {
        #[arg(long)]
        id: String,
        #[arg(long)]
        blocked_by: String,
        #[arg(long)]
        actor: String,
    },
    AddWorklog(Note),
    AddComment(Note),
    SetTitle {
        #[arg(long)]
        id: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        actor: String,
    },
    SetDescription {
        #[arg(long)]
        id: String,
        #[arg(long)]
        description: String,
        #[arg(long)]
        actor: String,
    },
    Assign {
        #[arg(long)]
        id: String,
        #[arg(long)]
        owner: String,
        #[arg(long)]
        actor: String,
    },
    SetReviewer {
        #[arg(long)]
        id: String,
        #[arg(long)]
        reviewer: String,
        #[arg(long)]
        actor: String,
    },
    SetDue {
        #[arg(long)]
        id: String,
        #[arg(long)]
        due_at: String,
        #[arg(long)]
        actor: String,
    },
    SetReviewRequired {
        #[arg(long)]
        id: String,
        /// `ArgAction::Set` so this reads `--value true`, matching the documented surface. A
        /// bare `bool` would make it a flag and reject the explicit value.
        #[arg(long, action = clap::ArgAction::Set)]
        value: bool,
        #[arg(long)]
        actor: String,
    },
    RemoveBlocker {
        #[arg(long)]
        id: String,
        #[arg(long)]
        blocked_by: String,
        #[arg(long)]
        actor: String,
    },
    /// Recurrence schedule for a task.
    SetRecurrence {
        #[arg(long)]
        id: String,
        #[arg(long, value_parser = ["hourly", "daily", "weekly", "monthly"])]
        frequency: String,
        #[arg(long, default_value_t = 1)]
        interval: u32,
        #[arg(long, value_parser = ["none", "relative", "absolute"], default_value = "none")]
        due_strategy: String,
        #[arg(long)]
        actor: String,
    },
    ClearRecurrence {
        #[arg(long)]
        id: String,
        #[arg(long)]
        actor: String,
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
        #[arg(long)]
        id: String,
        #[arg(long)]
        actor: String,
    },
    SoftDelete {
        #[arg(long)]
        id: String,
        #[arg(long)]
        actor: String,
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
    }
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
            actor: _,
            description,
            review_required,
            parent,
        } => {
            let label = "task create";
            if !load_owners(root).iter().any(|o| o.name == owner) {
                Envelope::err(
                    label,
                    "OWNER_NOT_FOUND",
                    format!("owner not in registry: {owner}"),
                )
                .emit();
            }
            let id = match store.next_id() {
                Ok(id) => id,
                Err(e) => Envelope::err(label, "IO_ERROR", e.to_string()).emit(),
            };
            if let Some(p) = &parent {
                if store.get(p).is_none() {
                    fail(label, TaskError::NotFound(p.clone()));
                }
            }
            let mut task = Task::new(&id, &title, workspace, &owner, now());
            task.description = description;
            task.review_required = review_required;
            task.parent_task_id = parent;
            store.put(task.clone());
            Envelope::ok(label, serde_json::to_value(task).unwrap_or_default()).emit()
        }

        TaskCmd::Show { id } => {
            let label = "task show";
            match store.get(&id) {
                Some(t) => Envelope::ok(label, serde_json::to_value(t).unwrap_or_default()).emit(),
                None => fail(label, TaskError::NotFound(id)),
            }
        }

        TaskCmd::List { status, archived } => {
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

        TaskCmd::SetReview {
            id,
            actor: _,
            review_id,
            expected_by,
            worker,
        } => {
            let label = "task set-review";
            let Some(mut task) = store.get(&id) else {
                fail(label, TaskError::NotFound(id))
            };
            // Absent flags leave the current value alone, so one field can be set without
            // clearing the other two.
            if review_id.is_some() {
                task.review_id = review_id;
            }
            if expected_by.is_some() {
                task.expected_by = expected_by;
            }
            if worker.is_some() {
                task.worker = worker;
            }
            task.updated_at = now();
            task.version += 1;
            store.put(task.clone());
            Envelope::ok(label, serde_json::to_value(task).unwrap_or_default()).emit()
        }

        TaskCmd::AddBlocker {
            id,
            blocked_by,
            actor: _,
        } => {
            let label = "task add-blocker";
            let Some(mut task) = store.get(&id) else {
                fail(label, TaskError::NotFound(id))
            };
            if store.get(&blocked_by).is_none() {
                fail(label, TaskError::NotFound(blocked_by));
            }
            if !task.blocked_by.contains(&blocked_by) {
                task.blocked_by.push(blocked_by);
            }
            task.updated_at = now();
            task.version += 1;
            store.put(task.clone());
            Envelope::ok(label, serde_json::to_value(task).unwrap_or_default()).emit()
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

        TaskCmd::SetTitle {
            id,
            title,
            actor: _,
        } => patch("task set-title", &mut store, &id, |t| t.title = title),
        TaskCmd::SetDescription {
            id,
            description,
            actor: _,
        } => patch("task set-description", &mut store, &id, |t| {
            t.description = Some(description)
        }),
        TaskCmd::Assign {
            id,
            owner,
            actor: _,
        } => {
            let label = "task assign";
            if !load_owners(root).iter().any(|o| o.name == owner) {
                Envelope::err(
                    label,
                    "OWNER_NOT_FOUND",
                    format!("owner not in registry: {owner}"),
                )
                .emit();
            }
            patch(label, &mut store, &id, |t| t.owner = owner)
        }
        TaskCmd::SetReviewer {
            id,
            reviewer,
            actor: _,
        } => {
            let label = "task set-reviewer";
            if !load_owners(root).iter().any(|o| o.name == reviewer) {
                Envelope::err(
                    label,
                    "OWNER_NOT_FOUND",
                    format!("owner not in registry: {reviewer}"),
                )
                .emit();
            }
            patch(label, &mut store, &id, |t| t.reviewer = Some(reviewer))
        }
        TaskCmd::SetDue {
            id,
            due_at,
            actor: _,
        } => patch("task set-due", &mut store, &id, |t| t.due_at = Some(due_at)),
        TaskCmd::SetReviewRequired {
            id,
            value,
            actor: _,
        } => patch("task set-review-required", &mut store, &id, |t| {
            t.review_required = value
        }),
        TaskCmd::RemoveBlocker {
            id,
            blocked_by,
            actor: _,
        } => patch("task remove-blocker", &mut store, &id, |t| {
            t.blocked_by.retain(|b| b != &blocked_by)
        }),
        TaskCmd::SetRecurrence {
            id,
            frequency,
            interval,
            due_strategy,
            actor: _,
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
            patch("task set-recurrence", &mut store, &id, |t| {
                t.recurrence = Some(Recurrence {
                    frequency: freq,
                    interval,
                    preserve_review_required: true,
                    carry_forward_description: true,
                    carry_forward_owner: true,
                    carry_forward_due_strategy: strat,
                })
            })
        }
        TaskCmd::ClearRecurrence { id, actor: _ } => {
            patch("task clear-recurrence", &mut store, &id, |t| {
                t.recurrence = None
            })
        }
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
        TaskCmd::Archive { id, actor: _ } => {
            let label = "task archive";
            match store.get(&id) {
                Some(t) if t.archived => Envelope::err(
                    label,
                    "ALREADY_ARCHIVED",
                    format!("{id} is already archived"),
                )
                .emit(),
                Some(_) => patch(label, &mut store, &id, |t| t.archived = true),
                None => fail(label, TaskError::NotFound(id)),
            }
        }
        TaskCmd::SoftDelete { id, actor: _ } => patch("task soft-delete", &mut store, &id, |t| {
            t.soft_deleted = true
        }),
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
            Envelope::ok(label, data).emit()
        }
        Err(e) => fail(label, e),
    }
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
    patch(label, store, &f.id, |t| {
        if kind == "attachments" {
            t.attachment_refs.push(reference);
        } else {
            t.artifact_refs.push(reference);
        }
    })
}

/// Read, mutate, and write back a task, bumping its version. Used by every patch-style
/// setter, so they all agree on the version/timestamp bookkeeping.
fn patch(label: &str, store: &mut FsStore, id: &str, f: impl FnOnce(&mut Task)) -> ! {
    let Some(mut task) = store.get(id) else {
        fail(label, TaskError::NotFound(id.to_string()))
    };
    f(&mut task);
    task.updated_at = now();
    task.version += 1;
    store.put(task.clone());
    Envelope::ok(label, serde_json::to_value(task).unwrap_or_default()).emit()
}
