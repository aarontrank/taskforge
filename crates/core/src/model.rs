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

/// What sort of work a task is, for reporting.
///
/// A closed set on purpose. Free text drifts into `bug`, `bugfix` and `Bug`, and the drift only
/// shows up as a wrong number in a report months later, when nobody remembers which was which.
/// Adding a variant here is cheap; renaming one is a migration, because stored tasks carry the
/// wire string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskKind {
    #[serde(rename = "feature")]
    Feature,
    #[serde(rename = "bug")]
    Bug,
    #[serde(rename = "chore")]
    Chore,
    #[serde(rename = "investigation")]
    Investigation,
    #[serde(rename = "oncall")]
    Oncall,
    #[serde(rename = "doc")]
    Doc,
}

/// A kind string that is not one of the known values.
#[derive(Debug, thiserror::Error)]
#[error("unknown task kind: {0}")]
pub struct UnknownKind(pub String);

impl TaskKind {
    /// Every kind. The one list the parser, the CLI's help, and callers share.
    pub const ALL: [TaskKind; 6] = [
        TaskKind::Feature,
        TaskKind::Bug,
        TaskKind::Chore,
        TaskKind::Investigation,
        TaskKind::Oncall,
        TaskKind::Doc,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            TaskKind::Feature => "feature",
            TaskKind::Bug => "bug",
            TaskKind::Chore => "chore",
            TaskKind::Investigation => "investigation",
            TaskKind::Oncall => "oncall",
            TaskKind::Doc => "doc",
        }
    }

    /// Parse a wire name. Unknown input is refused, never coerced — a miscategorised task is a
    /// wrong report, and this is the only place to catch it.
    pub fn parse(s: &str) -> Result<TaskKind, UnknownKind> {
        TaskKind::ALL
            .into_iter()
            .find(|k| k.as_str() == s)
            .ok_or_else(|| UnknownKind(s.to_string()))
    }
}

impl std::fmt::Display for TaskKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Parse an age such as `12h`, `7d` or `2w`. A bare number means days.
///
/// `None` for anything else, including zero and negatives: `--stale 0` would report every task
/// as abandoned, which is worse than refusing the input.
pub fn parse_age(s: &str) -> Option<time::Duration> {
    let (digits, unit) = match s.strip_suffix(['h', 'd', 'w']) {
        Some(rest) => (rest, s.as_bytes()[s.len() - 1]),
        None => (s, b'd'),
    };
    let n: i64 = digits.parse().ok()?;
    if n <= 0 {
        return None;
    }
    Some(match unit {
        b'h' => time::Duration::hours(n),
        b'w' => time::Duration::weeks(n),
        _ => time::Duration::days(n),
    })
}

/// Parse a stored date: a full RFC3339 instant, or a bare `YYYY-MM-DD` read as midnight UTC.
///
/// **Every reader and every write-side validator goes through this one function.** Two parsers
/// is how this went wrong: the CLI stored whatever string it was handed while `is_overdue` and
/// the recurrence arithmetic each demanded strict RFC3339 separately, so `--expected-by
/// 2026-09-08` was accepted and then permanently invisible to `--overdue`. A value that fails
/// here must be refused at the boundary rather than stored, because nothing downstream can
/// distinguish "unreadable" from "not late".
pub fn parse_date(s: &str) -> Option<time::OffsetDateTime> {
    if let Ok(t) = time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339) {
        return Some(t);
    }
    let date_only = time::macros::format_description!("[year]-[month]-[day]");
    let d = time::Date::parse(s, date_only).ok()?;
    Some(d.midnight().assume_utc())
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
/// `reviews`, `expected_by` and `worker` are the additions that let a task express an
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
    /// Reviews gating this task — a code-review id, a pull-request number, or a URL.
    ///
    /// Plural because one task can span several packages, each needing its own review, so a
    /// single field cannot represent the work honestly.
    ///
    /// `alias` and the custom deserializer together accept the older singular `review_id` key
    /// and fold it into a one-element list, so task files written before this was plural still
    /// load. Serialization always writes the plural key, so reading and writing a task migrates
    /// it.
    #[serde(default, alias = "review_id", deserialize_with = "one_or_many")]
    pub reviews: Vec<String>,
    /// When the current wait stops being "on schedule" and becomes overdue.
    #[serde(default)]
    pub expected_by: Option<String>,
    /// Worker holding this task, e.g. a tmux session name.
    #[serde(default)]
    pub worker: Option<String>,
    /// Development checkout the work happens in — a numbered dev workspace, a git
    /// worktree name, a sandbox id.
    ///
    /// Deliberately **not** `workspace`, which on this struct means the taskforge partition
    /// (`main`, `side`) that the task is filed under. They are different things that both get
    /// called "workspace" in conversation: putting a per-stream checkout id in `workspace`
    /// would file every stream in its own partition and break listing.
    #[serde(default)]
    pub checkout: Option<String>,
    /// What sort of work this is. `None` means unclassified, which a report counts as its own
    /// bucket rather than guessing.
    #[serde(default)]
    pub kind: Option<TaskKind>,
    /// Open-ended labels — a project, a theme, a component. Where `kind` is a closed set for
    /// counting, these absorb everything that would otherwise churn the schema.
    #[serde(default)]
    pub tags: Vec<String>,
    /// The external tracker item this task delivers, if any. An opaque string: taskforge does
    /// not know or care which tracker it came from.
    #[serde(default)]
    pub ticket: Option<String>,
    #[serde(default = "one_i64")]
    pub version: i64,
}

/// Accept either a single string or a list of them, so a legacy scalar `review_id` and a modern
/// `reviews` list both deserialize into the same field.
fn one_or_many<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    // `Option` so an explicit `review_id: null` in an old file reads as "no reviews".
    Ok(match Option::<OneOrMany>::deserialize(deserializer)? {
        None => Vec::new(),
        Some(OneOrMany::One(s)) => vec![s],
        Some(OneOrMany::Many(v)) => v,
    })
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
            reviews: Vec::new(),
            expected_by: None,
            worker: None,
            checkout: None,
            kind: None,
            tags: Vec::new(),
            ticket: None,
            version: 1,
        }
    }

    /// Whether this task's wait has passed the point it was expected to end.
    ///
    /// A finished task is never overdue however long ago its window lapsed — surfacing it would
    /// be noise, and the point of this query is work that needs attention now.
    pub fn is_overdue(&self, now: &str) -> bool {
        if self.status.is_terminal() {
            return false;
        }
        match (
            self.expected_by.as_deref().and_then(parse_date),
            parse_date(now),
        ) {
            (Some(expected), Some(now)) => now > expected,
            // An unreadable date is not evidence of lateness.
            _ => false,
        }
    }

    /// Whether this task has sat untouched for longer than `max_age`.
    ///
    /// Excludes the terminal statuses for the same reason as `is_overdue`: a task finished in
    /// January has not moved since, and that is correct rather than abandoned. `Merged` is
    /// deliberately *not* excluded — it is the state that rots silently while waiting to be
    /// accepted, which is exactly what this query is for.
    pub fn is_stale(&self, now: &str, max_age: time::Duration) -> bool {
        if self.status.is_terminal() {
            return false;
        }
        match (parse_date(&self.updated_at), parse_date(now)) {
            (Some(updated), Some(now)) => now - updated > max_age,
            _ => false,
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
    fn every_kind_round_trips_through_its_wire_name() {
        let cases = [
            (TaskKind::Feature, "feature"),
            (TaskKind::Bug, "bug"),
            (TaskKind::Chore, "chore"),
            (TaskKind::Investigation, "investigation"),
            (TaskKind::Oncall, "oncall"),
            (TaskKind::Doc, "doc"),
        ];
        assert_eq!(
            cases.len(),
            TaskKind::ALL.len(),
            "a new kind was not added here"
        );
        for (kind, wire) in cases {
            assert_eq!(kind.as_str(), wire, "as_str for {kind:?}");
            assert_eq!(TaskKind::parse(wire).unwrap(), kind, "parse of {wire}");
        }
    }

    #[test]
    fn an_unknown_kind_is_a_typed_error_not_a_silent_default() {
        // The whole value of a closed set is that `bugfix` is refused rather than counted as
        // its own category at report time.
        let err = TaskKind::parse("bugfix").unwrap_err();
        assert!(err.to_string().contains("bugfix"), "got: {err}");
    }

    #[test]
    fn a_new_task_carries_no_classification_yet() {
        let t = Task::new("TASK-0001", "t", "main", "a", "2026-09-03T00:00:00Z");
        assert!(
            t.kind.is_none(),
            "kind is opt-in, not guessed at create time"
        );
        assert!(t.ticket.is_none());
        assert!(t.tags.is_empty());
        assert!(t.reviews.is_empty());
    }

    /// Task files written before `reviews` became plural carry a singular `review_id`.
    #[test]
    fn a_legacy_singular_review_id_loads_as_a_one_element_review_list() {
        let legacy = "\
id: TASK-0001
title: legacy
status: open
workspace: main
owner: agent
created_at: 2026-04-01T00:00:00Z
updated_at: 2026-04-01T00:00:00Z
review_id: PR-4821
";
        let t: Task = serde_yaml::from_str(legacy).expect("a legacy task file still loads");
        assert_eq!(
            t.reviews,
            vec!["PR-4821".to_string()],
            "the old single review is preserved, not dropped"
        );
    }

    #[test]
    fn a_plural_reviews_list_loads_as_itself() {
        let modern = "\
id: TASK-0001
title: modern
status: open
workspace: main
owner: agent
created_at: 2026-09-03T00:00:00Z
updated_at: 2026-09-03T00:00:00Z
reviews:
- PR-1
- PR-2
";
        let t: Task = serde_yaml::from_str(modern).expect("loads");
        assert_eq!(t.reviews, vec!["PR-1".to_string(), "PR-2".to_string()]);
    }

    #[test]
    fn a_task_with_no_review_at_all_loads_with_an_empty_list() {
        let bare = "\
id: TASK-0001
title: bare
status: open
workspace: main
owner: agent
created_at: 2026-09-03T00:00:00Z
updated_at: 2026-09-03T00:00:00Z
";
        let t: Task = serde_yaml::from_str(bare).expect("loads");
        assert!(t.reviews.is_empty());
    }

    #[test]
    fn writing_a_task_back_uses_the_plural_key_so_the_legacy_form_migrates() {
        let mut t = Task::new("TASK-0001", "t", "main", "a", "2026-09-03T00:00:00Z");
        t.reviews = vec!["PR-1".into()];
        let out = serde_yaml::to_string(&t).unwrap();
        assert!(out.contains("reviews:"), "plural key is written: {out}");
        assert!(
            !out.contains("review_id:"),
            "the legacy key is not written back: {out}"
        );
    }

    #[test]
    fn an_overdue_task_is_one_whose_expected_by_has_passed() {
        let mut t = Task::new("TASK-0001", "t", "main", "a", "2026-09-01T00:00:00Z");
        t.status = TaskStatus::InReview;
        t.expected_by = Some("2026-09-02T00:00:00Z".into());
        assert!(t.is_overdue("2026-09-03T00:00:00Z"), "past its window");
        assert!(!t.is_overdue("2026-09-01T12:00:00Z"), "still inside it");
    }

    #[test]
    fn a_task_with_no_expected_by_is_never_overdue() {
        let mut t = Task::new("TASK-0001", "t", "main", "a", "2026-09-01T00:00:00Z");
        t.status = TaskStatus::InReview;
        assert!(!t.is_overdue("2027-01-01T00:00:00Z"));
    }

    #[test]
    fn a_date_only_expected_by_is_read_as_midnight_utc() {
        // The CLI accepts `--expected-by 2026-09-08` and stores it verbatim, so the reader has
        // to understand what the writer was allowed to write. A strict RFC3339 parse made every
        // such task permanently invisible to `--overdue` — the whole point of the query.
        let mut t = Task::new("TASK-0001", "t", "main", "a", "2026-09-01T00:00:00Z");
        t.status = TaskStatus::InReview;
        t.expected_by = Some("2026-09-08".into());
        assert!(t.is_overdue("2026-09-09T00:00:00Z"), "the day has passed");
        assert!(
            !t.is_overdue("2026-09-08T00:00:00Z"),
            "midnight itself is the start of the window, not past it"
        );
    }

    #[test]
    fn an_unreadable_expected_by_is_still_not_evidence_of_lateness() {
        // Leniency stops at the two forms a caller can actually write. Anything else stays
        // "unknown", never "late".
        let mut t = Task::new("TASK-0001", "t", "main", "a", "2026-09-01T00:00:00Z");
        t.status = TaskStatus::InReview;
        t.expected_by = Some("next tuesday".into());
        assert!(!t.is_overdue("2027-01-01T00:00:00Z"));
    }

    #[test]
    fn a_finished_task_is_never_overdue_however_old_its_window() {
        // A done task with a long-past expected_by is not a problem to surface.
        for status in [TaskStatus::Done, TaskStatus::Failed, TaskStatus::Cancelled] {
            let mut t = Task::new("TASK-0001", "t", "main", "a", "2026-01-01T00:00:00Z");
            t.status = status;
            t.expected_by = Some("2026-01-02T00:00:00Z".into());
            assert!(
                !t.is_overdue("2026-09-03T00:00:00Z"),
                "{status} must not be overdue"
            );
        }
    }

    #[test]
    fn a_stale_task_is_one_that_has_not_moved_for_longer_than_the_limit() {
        let mut t = Task::new("TASK-0001", "t", "main", "a", "2026-09-01T00:00:00Z");
        t.status = TaskStatus::InReview;
        t.updated_at = "2026-09-01T00:00:00Z".into();
        let two_days = time::Duration::days(2);
        assert!(
            t.is_stale("2026-09-04T00:00:00Z", two_days),
            "three days idle"
        );
        assert!(
            !t.is_stale("2026-09-02T00:00:00Z", two_days),
            "one day idle"
        );
    }

    #[test]
    fn a_finished_task_is_never_stale() {
        // Its last update was when it finished, which is correct rather than abandoned.
        let mut t = Task::new("TASK-0001", "t", "main", "a", "2026-01-01T00:00:00Z");
        t.status = TaskStatus::Done;
        t.updated_at = "2026-01-01T00:00:00Z".into();
        assert!(!t.is_stale("2026-09-03T00:00:00Z", time::Duration::days(1)));
    }

    #[test]
    fn a_merged_task_can_go_stale_because_it_is_waiting_on_a_human() {
        // The reason merged is excluded from is_terminal: it is exactly the state that rots
        // silently while waiting to be accepted.
        let mut t = Task::new("TASK-0001", "t", "main", "a", "2026-08-01T00:00:00Z");
        t.status = TaskStatus::Merged;
        t.updated_at = "2026-08-01T00:00:00Z".into();
        assert!(t.is_stale("2026-09-03T00:00:00Z", time::Duration::days(7)));
    }

    #[test]
    fn an_unparseable_timestamp_is_not_treated_as_stale_or_overdue() {
        // A corrupt date must not silently flood the stale list.
        let mut t = Task::new("TASK-0001", "t", "main", "a", "not-a-date");
        t.status = TaskStatus::Running;
        t.updated_at = "not-a-date".into();
        t.expected_by = Some("also-not-a-date".into());
        assert!(!t.is_stale("2026-09-03T00:00:00Z", time::Duration::days(1)));
        assert!(!t.is_overdue("2026-09-03T00:00:00Z"));
    }

    #[test]
    fn an_age_is_parsed_from_hours_days_or_weeks() {
        assert_eq!(parse_age("12h"), Some(time::Duration::hours(12)));
        assert_eq!(parse_age("7d"), Some(time::Duration::days(7)));
        assert_eq!(parse_age("2w"), Some(time::Duration::weeks(2)));
    }

    #[test]
    fn a_bare_number_is_read_as_days() {
        assert_eq!(parse_age("7"), Some(time::Duration::days(7)));
    }

    #[test]
    fn a_nonsense_age_is_refused_rather_than_defaulted() {
        for bad in ["", "d", "7y", "-3d", "abc", "7dd"] {
            assert_eq!(parse_age(bad), None, "{bad:?} must not parse");
        }
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
        assert!(t.reviews.is_empty(), "reviews starts empty");
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
        t.reviews = vec!["PR-4821".into()];
        t.expected_by = Some("2026-09-03T17:00:00Z".into());
        t.worker = Some("review-worker-1".into());

        let yaml = serde_yaml::to_string(&t).expect("serialize");
        let back: Task = serde_yaml::from_str(&yaml).expect("deserialize");

        assert_eq!(back.status, TaskStatus::InReview);
        assert_eq!(back.reviews, vec!["PR-4821".to_string()]);
        assert_eq!(back.expected_by.as_deref(), Some("2026-09-03T17:00:00Z"));
        assert_eq!(back.worker.as_deref(), Some("review-worker-1"));
    }

    #[test]
    fn status_is_written_in_wire_form_not_as_a_rust_variant_name() {
        let mut t = Task::new("TASK-0003", "x", "main", "a", "2026-09-01T00:00:00Z");
        t.status = TaskStatus::ChangesRequested;
        let yaml = serde_yaml::to_string(&t).unwrap();
        assert!(yaml.contains("status: changes-requested"), "got:\n{yaml}");
    }
}
