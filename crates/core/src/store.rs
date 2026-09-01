use crate::model::Task;
use serde::{Deserialize, Serialize};

/// One line of a task's append-only `audit.log`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub ts: String,
    pub actor: String,
    pub action: String,
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
}

/// Read/write access to tasks and their audit trail.
///
/// A trait rather than a concrete filesystem type so the domain logic can be developed and
/// tested against an in-memory implementation, with no temp directories and no I/O in the
/// unit tests that cover the status machine.
pub trait TaskStore {
    fn get(&self, id: &str) -> Option<Task>;
    fn put(&mut self, task: Task);
    fn append_audit(&mut self, entry: AuditEntry);
}

/// In-memory store for tests.
#[derive(Debug, Default)]
pub struct MemoryStore {
    tasks: std::collections::BTreeMap<String, Task>,
    audit: Vec<AuditEntry>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Audit entries recorded for one task, in order.
    pub fn audit_of(&self, id: &str) -> Vec<AuditEntry> {
        self.audit
            .iter()
            .filter(|e| e.task_id == id)
            .cloned()
            .collect()
    }
}

impl TaskStore for MemoryStore {
    fn get(&self, id: &str) -> Option<Task> {
        self.tasks.get(id).cloned()
    }

    fn put(&mut self, task: Task) {
        self.tasks.insert(task.id.clone(), task);
    }

    fn append_audit(&mut self, entry: AuditEntry) {
        self.audit.push(entry);
    }
}
