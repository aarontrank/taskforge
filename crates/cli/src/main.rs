//! taskforge CLI.
//!
//! Every command prints the documented JSON envelope and exits non-zero on error, so an agent
//! can branch on the exit status and read `errors[0].code` without parsing prose.

use clap::{Parser, Subcommand};
use serde::Serialize;
use std::path::PathBuf;
use taskforge_core::fsstore::FsStore;
use taskforge_core::model::{Owner, OwnerType, Task, TaskStatus};
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
    },
    Show {
        #[arg(long)]
        id: String,
    },
    List {
        #[arg(long)]
        status: Option<String>,
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
            let mut task = Task::new(&id, &title, workspace, &owner, now());
            task.description = description;
            task.review_required = review_required;
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

        TaskCmd::List { status } => {
            let label = "task list";
            let dir = root.join("workspaces").join(workspace).join("tasks");
            let mut tasks: Vec<Task> = std::fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|e| e.file_name().to_str().map(String::from))
                .filter_map(|id| store.get(&id))
                .collect();
            if let Some(want) = status {
                match TaskStatus::parse(&want) {
                    Ok(s) => tasks.retain(|t| t.status == s),
                    Err(e) => Envelope::err(label, "INVALID_STATUS", e.to_string()).emit(),
                }
            }
            tasks.sort_by(|a, b| a.id.cmp(&b.id));
            Envelope::ok(label, serde_json::to_value(tasks).unwrap_or_default()).emit()
        }

        TaskCmd::Start(a) => transition("task start", store, a, TaskStatus::Running),
        TaskCmd::RequestReview(a) => {
            transition("task request-review", store, a, TaskStatus::InReview)
        }
        TaskCmd::Merge(a) => transition("task merge", store, a, TaskStatus::Merged),
        TaskCmd::Accept(a) => transition("task accept", store, a, TaskStatus::Done),
        TaskCmd::Complete(a) => transition("task complete", store, a, TaskStatus::Done),
        TaskCmd::Reject(a) => transition("task reject", store, a, TaskStatus::ChangesRequested),

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
    }
}

fn transition(label: &str, store: FsStore, a: Act, to: TaskStatus) -> ! {
    let mut svc = TaskService::new(store, now());
    match svc.set_status(&a.id, to, &a.actor, a.version) {
        Ok(t) => Envelope::ok(label, serde_json::to_value(t).unwrap_or_default()).emit(),
        Err(e) => fail(label, e),
    }
}
