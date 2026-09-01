use serde::{Deserialize, Serialize};

/// Execution state of a task.
///
/// These are the eleven values the `orchestrate` skill's status board uses, so a taskforge
/// task can express the distinctions that board exists to make. Two matter most:
///
/// * `Merged` is **not** terminal. A merged review still awaits human acceptance.
/// * `Done` means merged **and** accepted. It is the only success terminal.
///
/// `Open` is retained as the initial state a freshly created task carries; `Pending` is the
/// planned-but-not-dispatched state used once a task is part of a wave.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    #[serde(rename = "open")]
    Open,
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "in-review")]
    InReview,
    #[serde(rename = "changes-requested")]
    ChangesRequested,
    #[serde(rename = "merged")]
    Merged,
    #[serde(rename = "waiting-on-schedule")]
    WaitingOnSchedule,
    #[serde(rename = "stuck")]
    Stuck,
    #[serde(rename = "done")]
    Done,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "cancelled")]
    Cancelled,
}

/// A status string that is not one of the eleven known values.
#[derive(Debug, thiserror::Error)]
#[error("unknown task status: {0}")]
pub struct UnknownStatus(pub String);

impl TaskStatus {
    /// Every status, in board order. The single list the parser and callers share, so a new
    /// variant cannot be added to one and forgotten in the other.
    pub const ALL: [TaskStatus; 11] = [
        TaskStatus::Open,
        TaskStatus::Pending,
        TaskStatus::Running,
        TaskStatus::InReview,
        TaskStatus::ChangesRequested,
        TaskStatus::Merged,
        TaskStatus::WaitingOnSchedule,
        TaskStatus::Stuck,
        TaskStatus::Done,
        TaskStatus::Failed,
        TaskStatus::Cancelled,
    ];

    /// The wire name, as written to `task.md` frontmatter and printed by the CLI.
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Open => "open",
            TaskStatus::Pending => "pending",
            TaskStatus::Running => "running",
            TaskStatus::InReview => "in-review",
            TaskStatus::ChangesRequested => "changes-requested",
            TaskStatus::Merged => "merged",
            TaskStatus::WaitingOnSchedule => "waiting-on-schedule",
            TaskStatus::Stuck => "stuck",
            TaskStatus::Done => "done",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "cancelled",
        }
    }

    /// Parse a wire name. Unknown input is a typed error, never a panic or a silent default:
    /// a task file with a status this build does not understand must be reported, not guessed.
    pub fn parse(s: &str) -> Result<TaskStatus, UnknownStatus> {
        TaskStatus::ALL
            .into_iter()
            .find(|c| c.as_str() == s)
            .ok_or_else(|| UnknownStatus(s.to_string()))
    }

    /// Whether no further transition is possible. `Merged` is deliberately excluded — it
    /// awaits acceptance, and treating it as finished is the mistake this model prevents.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            TaskStatus::Done | TaskStatus::Failed | TaskStatus::Cancelled
        )
    }
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How a recurring task generates its next occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecurrenceFrequency {
    Hourly,
    Daily,
    Weekly,
    Monthly,
}

/// What to do with the due date when rolling a recurring task forward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DueStrategy {
    #[default]
    None,
    Relative,
    Absolute,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recurrence {
    pub frequency: RecurrenceFrequency,
    #[serde(default = "one")]
    pub interval: u32,
    #[serde(default = "yes")]
    pub preserve_review_required: bool,
    #[serde(default = "yes")]
    pub carry_forward_description: bool,
    #[serde(default = "yes")]
    pub carry_forward_owner: bool,
    #[serde(default)]
    pub carry_forward_due_strategy: DueStrategy,
}

fn one() -> u32 {
    1
}
fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReviewOutcome {
    Approved,
    Rejected,
}

/// A task. Serialized as the YAML frontmatter of `task.md`; the markdown body is held
/// separately by the storage layer, not here.
///
/// `review_id`, `expected_by` and `worker` are the additions that let a task express an
/// `orchestrate` board row: which review it is waiting on, when that wait becomes overdue,
/// and which worker owns it. Without `expected_by` the difference between "in review, on
/// schedule" and "stuck" is a judgement call rather than data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub status: TaskStatus,
    pub workspace: String,
    pub owner: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub review_required: bool,
    #[serde(default)]
    pub soft_deleted: bool,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub due_at: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
    #[serde(default)]
    pub review_requested_at: Option<String>,
    #[serde(default)]
    pub reviewed_at: Option<String>,
    #[serde(default)]
    pub reviewer: Option<String>,
    #[serde(default)]
    pub review_outcome: Option<ReviewOutcome>,
    #[serde(default)]
    pub parent_task_id: Option<String>,
    #[serde(default)]
    pub blocked_by: Vec<String>,
    #[serde(default)]
    pub recurrence: Option<Recurrence>,
    #[serde(default)]
    pub prior_occurrence_id: Option<String>,
    #[serde(default)]
    pub attachment_refs: Vec<String>,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    /// Review this task is gated on — a CR id, a PR number, or a URL.
    #[serde(default)]
    pub review_id: Option<String>,
    /// When the current wait stops being "on schedule" and becomes overdue.
    #[serde(default)]
    pub expected_by: Option<String>,
    /// Worker holding this task, e.g. a tmux session name.
    #[serde(default)]
    pub worker: Option<String>,
    /// Development checkout the work happens in — an `imdb-next-gen` workspace number, a git
    /// worktree name, a sandbox id.
    ///
    /// Deliberately **not** `workspace`, which on this struct means the taskforge partition
    /// (`main`, `side`) that the task is filed under. They are different things that both get
    /// called "workspace" in conversation: putting a per-stream checkout id in `workspace`
    /// would file every stream in its own partition and break listing.
    #[serde(default)]
    pub checkout: Option<String>,
    #[serde(default = "one_i64")]
    pub version: i64,
}

fn one_i64() -> i64 {
    1
}

impl Task {
    /// A freshly created task: `Open`, version 1, no review or scheduling state yet.
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        workspace: impl Into<String>,
        owner: impl Into<String>,
        now: impl Into<String>,
    ) -> Self {
        let now = now.into();
        Task {
            id: id.into(),
            title: title.into(),
            status: TaskStatus::Open,
            workspace: workspace.into(),
            owner: owner.into(),
            created_at: now.clone(),
            updated_at: now,
            review_required: false,
            soft_deleted: false,
            archived: false,
            description: None,
            due_at: None,
            completed_at: None,
            review_requested_at: None,
            reviewed_at: None,
            reviewer: None,
            review_outcome: None,
            parent_task_id: None,
            blocked_by: Vec::new(),
            recurrence: None,
            prior_occurrence_id: None,
            attachment_refs: Vec::new(),
            artifact_refs: Vec::new(),
            review_id: None,
            expected_by: None,
            worker: None,
            checkout: None,
            version: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OwnerType {
    Human,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Owner {
    pub name: String,
    #[serde(rename = "type")]
    pub owner_type: OwnerType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default = "yes")]
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[cfg(test)]
mod tests {
    use crate::model::*;

    // All eleven orchestrate statuses must exist and round-trip through their wire form.
    #[test]
    fn every_status_round_trips_through_its_wire_name() {
        let cases = [
            (TaskStatus::Pending, "pending"),
            (TaskStatus::Running, "running"),
            (TaskStatus::InReview, "in-review"),
            (TaskStatus::ChangesRequested, "changes-requested"),
            (TaskStatus::Merged, "merged"),
            (TaskStatus::WaitingOnSchedule, "waiting-on-schedule"),
            (TaskStatus::Stuck, "stuck"),
            (TaskStatus::Done, "done"),
            (TaskStatus::Failed, "failed"),
            (TaskStatus::Cancelled, "cancelled"),
            (TaskStatus::Open, "open"),
        ];
        for (status, wire) in cases {
            assert_eq!(status.as_str(), wire, "as_str for {status:?}");
            assert_eq!(TaskStatus::parse(wire).unwrap(), status, "parse of {wire}");
        }
    }

    #[test]
    fn an_unknown_status_is_a_typed_error_not_a_panic() {
        let err = TaskStatus::parse("in_progress").unwrap_err();
        assert!(err.to_string().contains("in_progress"), "got: {err}");
    }

    #[test]
    fn merged_is_not_terminal_but_done_is() {
        assert!(!TaskStatus::Merged.is_terminal());
        assert!(TaskStatus::Done.is_terminal());
        assert!(TaskStatus::Failed.is_terminal());
        assert!(TaskStatus::Cancelled.is_terminal());
        assert!(!TaskStatus::Running.is_terminal());
    }

    #[test]
    fn a_new_task_carries_the_orchestrate_fields_as_none() {
        let t = Task::new(
            "TASK-0001",
            "Port storage",
            "main",
            "agent",
            "2026-09-01T00:00:00Z",
        );
        assert_eq!(t.status, TaskStatus::Open);
        assert_eq!(t.version, 1);
        assert!(t.review_id.is_none(), "review_id starts unset");
        assert!(t.expected_by.is_none(), "expected_by starts unset");
        assert!(t.worker.is_none(), "worker starts unset");
        assert!(t.blocked_by.is_empty());
        assert!(!t.archived && !t.soft_deleted);
    }

    // The three new fields must survive a frontmatter round-trip, or the board data they
    // carry is silently lost on the next read.
    #[test]
    fn orchestrate_fields_survive_a_yaml_round_trip() {
        let mut t = Task::new(
            "TASK-0002",
            "Ship it",
            "main",
            "aaron",
            "2026-09-01T00:00:00Z",
        );
        t.status = TaskStatus::InReview;
        t.review_id = Some("CR-301625168".into());
        t.expected_by = Some("2026-09-03T17:00:00Z".into());
        t.worker = Some("addresscr-CR-301625168".into());

        let yaml = serde_yaml::to_string(&t).expect("serialize");
        let back: Task = serde_yaml::from_str(&yaml).expect("deserialize");

        assert_eq!(back.status, TaskStatus::InReview);
        assert_eq!(back.review_id.as_deref(), Some("CR-301625168"));
        assert_eq!(back.expected_by.as_deref(), Some("2026-09-03T17:00:00Z"));
        assert_eq!(back.worker.as_deref(), Some("addresscr-CR-301625168"));
    }

    #[test]
    fn status_is_written_in_wire_form_not_as_a_rust_variant_name() {
        let mut t = Task::new("TASK-0003", "x", "main", "a", "2026-09-01T00:00:00Z");
        t.status = TaskStatus::ChangesRequested;
        let yaml = serde_yaml::to_string(&t).unwrap();
        assert!(yaml.contains("status: changes-requested"), "got:\n{yaml}");
    }
}
