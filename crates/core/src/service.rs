use crate::model::{Task, TaskStatus};
use crate::store::{AuditEntry, TaskStore};
use crate::transition::can_transition;

/// A domain error, carrying the stable code the CLI reports in its JSON envelope.
#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    #[error("task not found: {0}")]
    NotFound(String),
    #[error("cannot move {id} from {from} to {to}")]
    InvalidTransition {
        id: String,
        from: TaskStatus,
        to: TaskStatus,
    },
    #[error("{id} is blocked by unresolved task(s): {blockers}")]
    Blocked { id: String, blockers: String },
    #[error("{0} requires review; request review instead of completing it directly")]
    ReviewRequired(String),
    #[error("{id} is at version {actual}, not {expected}; re-read and retry")]
    VersionMismatch {
        id: String,
        expected: i64,
        actual: i64,
    },
}

impl TaskError {
    /// The stable machine-readable code. Kept identical to the documented set so existing
    /// agent error handling keeps working across the rewrite.
    pub fn code(&self) -> &'static str {
        match self {
            TaskError::NotFound(_) => "TASK_NOT_FOUND",
            TaskError::InvalidTransition { .. } => "INVALID_STATUS_TRANSITION",
            TaskError::Blocked { .. } => "BLOCKED_BY_OPEN_TASK",
            TaskError::ReviewRequired(_) => "REVIEW_REQUIRED",
            TaskError::VersionMismatch { .. } => "CONFLICT_VERSION_MISMATCH",
        }
    }
}

/// Workflow operations over a store.
///
/// `now` is injected rather than read from the clock so every timestamp assertion in the
/// tests is exact instead of approximate.
pub struct TaskService<S: TaskStore> {
    pub store: S,
    now: String,
}

impl<S: TaskStore> TaskService<S> {
    pub fn new(store: S, now: impl Into<String>) -> Self {
        TaskService {
            store,
            now: now.into(),
        }
    }

    /// Move a task to a new status, enforcing the transition table, blockers, the review
    /// gate, and optimistic concurrency — in that order, so the most specific complaint wins.
    pub fn set_status(
        &mut self,
        id: &str,
        to: TaskStatus,
        actor: &str,
        expected_version: Option<i64>,
    ) -> Result<Task, TaskError> {
        let mut task = self
            .store
            .get(id)
            .ok_or_else(|| TaskError::NotFound(id.to_string()))?;

        if let Some(expected) = expected_version {
            if expected != task.version {
                return Err(TaskError::VersionMismatch {
                    id: id.to_string(),
                    expected,
                    actual: task.version,
                });
            }
        }

        let from = task.status;
        if !can_transition(from, to) {
            return Err(TaskError::InvalidTransition {
                id: id.to_string(),
                from,
                to,
            });
        }

        // Completing without review is only legitimate when no review was demanded.
        if to == TaskStatus::Done && task.review_required && from != TaskStatus::Merged {
            return Err(TaskError::ReviewRequired(id.to_string()));
        }

        // Dispatching work requires every dependency to be genuinely finished. `Merged` does
        // not qualify: it still awaits acceptance.
        if to == TaskStatus::Running {
            let unresolved: Vec<String> = task
                .blocked_by
                .iter()
                .filter(|b| {
                    self.store
                        .get(b)
                        .map(|t| t.status != TaskStatus::Done)
                        .unwrap_or(true)
                })
                .cloned()
                .collect();
            if !unresolved.is_empty() {
                return Err(TaskError::Blocked {
                    id: id.to_string(),
                    blockers: unresolved.join(", "),
                });
            }
        }

        task.status = to;
        task.updated_at = self.now.clone();
        task.version += 1;
        match to {
            TaskStatus::Done => task.completed_at = Some(self.now.clone()),
            TaskStatus::InReview => task.review_requested_at = Some(self.now.clone()),
            _ => {}
        }

        self.store.put(task.clone());
        self.store.append_audit(AuditEntry {
            ts: self.now.clone(),
            actor: actor.to_string(),
            action: "status_changed".to_string(),
            task_id: id.to_string(),
            from: Some(from.to_string()),
            to: Some(to.to_string()),
        });
        Ok(task)
    }
}

#[cfg(test)]
mod tests {
    use crate::model::{Task, TaskStatus};
    use crate::service::*;
    use crate::store::MemoryStore;

    fn task(id: &str) -> Task {
        Task::new(id, "t", "main", "agent", "2026-09-01T00:00:00Z")
    }

    fn svc() -> TaskService<MemoryStore> {
        TaskService::new(MemoryStore::new(), "2026-09-02T00:00:00Z")
    }

    #[test]
    fn start_refuses_while_a_blocker_is_unresolved() {
        let mut s = svc();
        s.store.put(task("TASK-0002"));
        let mut t = task("TASK-0001");
        t.blocked_by = vec!["TASK-0002".into()];
        s.store.put(t);

        let err = s
            .set_status("TASK-0001", TaskStatus::Running, "agent", None)
            .unwrap_err();
        assert_eq!(err.code(), "BLOCKED_BY_OPEN_TASK", "got: {err}");
        assert!(
            err.to_string().contains("TASK-0002"),
            "names the blocker: {err}"
        );
    }

    #[test]
    fn start_proceeds_once_every_blocker_is_done() {
        let mut s = svc();
        let mut b = task("TASK-0002");
        b.status = TaskStatus::Done;
        s.store.put(b);
        let mut t = task("TASK-0001");
        t.blocked_by = vec!["TASK-0002".into()];
        s.store.put(t);

        let out = s
            .set_status("TASK-0001", TaskStatus::Running, "agent", None)
            .unwrap();
        assert_eq!(out.status, TaskStatus::Running);
        assert_eq!(out.version, 2, "a mutation bumps the version");
        assert_eq!(out.updated_at, "2026-09-02T00:00:00Z");
    }

    #[test]
    fn a_merged_blocker_does_not_count_as_resolved() {
        // Merged awaits acceptance. Letting a dependent start on it is exactly the
        // premature-completion mistake the eleven-state model exists to prevent.
        let mut s = svc();
        let mut b = task("TASK-0002");
        b.status = TaskStatus::Merged;
        s.store.put(b);
        let mut t = task("TASK-0001");
        t.blocked_by = vec!["TASK-0002".into()];
        s.store.put(t);

        let err = s
            .set_status("TASK-0001", TaskStatus::Running, "agent", None)
            .unwrap_err();
        assert_eq!(err.code(), "BLOCKED_BY_OPEN_TASK");
    }

    #[test]
    fn an_illegal_transition_is_rejected_by_code() {
        let mut s = svc();
        s.store.put(task("TASK-0001")); // Open
        let err = s
            .set_status("TASK-0001", TaskStatus::Merged, "agent", None)
            .unwrap_err();
        assert_eq!(err.code(), "INVALID_STATUS_TRANSITION", "got: {err}");
    }

    #[test]
    fn completing_a_review_required_task_directly_is_refused() {
        let mut s = svc();
        let mut t = task("TASK-0001");
        t.status = TaskStatus::Running;
        t.review_required = true;
        s.store.put(t);

        let err = s
            .set_status("TASK-0001", TaskStatus::Done, "agent", None)
            .unwrap_err();
        assert_eq!(err.code(), "REVIEW_REQUIRED", "got: {err}");
    }

    #[test]
    fn a_stale_version_is_a_conflict() {
        let mut s = svc();
        s.store.put(task("TASK-0001")); // version 1
        let err = s
            .set_status("TASK-0001", TaskStatus::Running, "agent", Some(99))
            .unwrap_err();
        assert_eq!(err.code(), "CONFLICT_VERSION_MISMATCH", "got: {err}");
    }

    #[test]
    fn the_matching_version_is_accepted() {
        let mut s = svc();
        s.store.put(task("TASK-0001"));
        assert!(s
            .set_status("TASK-0001", TaskStatus::Running, "agent", Some(1))
            .is_ok());
    }

    #[test]
    fn an_unknown_task_is_reported_not_created() {
        let mut s = svc();
        let err = s
            .set_status("TASK-9999", TaskStatus::Running, "agent", None)
            .unwrap_err();
        assert_eq!(err.code(), "TASK_NOT_FOUND");
    }

    #[test]
    fn reaching_done_stamps_completed_at() {
        let mut s = svc();
        let mut t = task("TASK-0001");
        t.status = TaskStatus::Running;
        s.store.put(t);
        let out = s
            .set_status("TASK-0001", TaskStatus::Done, "agent", None)
            .unwrap();
        assert_eq!(out.completed_at.as_deref(), Some("2026-09-02T00:00:00Z"));
    }

    #[test]
    fn entering_review_stamps_the_request_time() {
        let mut s = svc();
        let mut t = task("TASK-0001");
        t.status = TaskStatus::Running;
        t.review_required = true;
        s.store.put(t);
        let out = s
            .set_status("TASK-0001", TaskStatus::InReview, "agent", None)
            .unwrap();
        assert_eq!(
            out.review_requested_at.as_deref(),
            Some("2026-09-02T00:00:00Z")
        );
    }

    #[test]
    fn every_mutation_appends_one_audit_entry() {
        let mut s = svc();
        s.store.put(task("TASK-0001"));
        s.set_status("TASK-0001", TaskStatus::Running, "aaron", None)
            .unwrap();
        let log = s.store.audit_of("TASK-0001");
        assert_eq!(log.len(), 1, "one entry per mutation");
        assert_eq!(log[0].actor, "aaron");
        assert_eq!(log[0].action, "status_changed");
    }
}
