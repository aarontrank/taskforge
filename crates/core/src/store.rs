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
///
/// Writes return `io::Result` because a store that cannot report a failed write forces its
/// callers to pretend one succeeded. Reads stay `Option`: the domain only ever needs to know
/// whether the task is there.
pub trait TaskStore {
    fn get(&self, id: &str) -> Option<Task>;
    fn put(&mut self, task: Task) -> std::io::Result<()>;
    fn append_audit(&mut self, entry: AuditEntry) -> std::io::Result<()>;
    /// Allocate the next task id. On the trait because generating a recurring task's next
    /// occurrence is domain logic that has to mint an id without knowing the storage kind.
    fn allocate_id(&mut self) -> String;
}

/// In-memory store for tests.
#[derive(Debug, Default)]
pub struct MemoryStore {
    tasks: std::collections::BTreeMap<String, Task>,
    audit: Vec<AuditEntry>,
    next: u64,
    /// Writes still permitted before every subsequent one fails. `None` means never fail.
    writes_allowed: Option<usize>,
    audit_fails: bool,
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

    /// Fail every write from now on.
    ///
    /// A test seam for the disk-full and read-only-mount paths, which are the ones that used
    /// to be silently swallowed and cannot otherwise be reached without root or a real disk.
    pub fn fail_writes_now(&mut self) {
        self.writes_allowed = Some(0);
    }

    /// Allow `n` further writes, then fail. Lets a test target the *second* write of an
    /// operation — e.g. a recurring successor, whose predecessor must still commit.
    pub fn fail_writes_after(&mut self, n: usize) {
        self.writes_allowed = Some(n);
    }

    /// Fail every audit append from now on, leaving task writes working.
    pub fn fail_audit_now(&mut self) {
        self.audit_fails = true;
    }

    fn refuse() -> std::io::Error {
        std::io::Error::other("write refused by test store")
    }
}

impl TaskStore for MemoryStore {
    fn get(&self, id: &str) -> Option<Task> {
        self.tasks.get(id).cloned()
    }

    fn put(&mut self, task: Task) -> std::io::Result<()> {
        match self.writes_allowed {
            Some(0) => return Err(Self::refuse()),
            Some(n) => self.writes_allowed = Some(n - 1),
            None => {}
        }
        self.tasks.insert(task.id.clone(), task);
        Ok(())
    }

    fn append_audit(&mut self, entry: AuditEntry) -> std::io::Result<()> {
        if self.audit_fails {
            return Err(Self::refuse());
        }
        self.audit.push(entry);
        Ok(())
    }

    fn allocate_id(&mut self) -> String {
        self.next += 1;
        format!("TASK-{:04}", self.next + 1000)
    }
}
