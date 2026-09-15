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
    /// Write a task only if the stored version is still `expected`.
    ///
    /// A caller reads a task, checks its version, does its work, and writes — and everything in
    /// between is a window another writer can land in. Both then compute the same next version,
    /// both report success, and the later write silently discards the earlier one. This narrows
    /// that window to the write itself.
    ///
    /// It is a narrowing, not a lock: [`FsStore`](crate::fsstore::FsStore) checks immediately
    /// before publishing the file, so the remaining gap is one `rename`. An absent task is
    /// refused — there is nothing to compare against, so no write here can be safe.
    ///
    /// Returns [`ErrorKind::AlreadyExists`](std::io::ErrorKind::AlreadyExists) on a mismatch,
    /// which callers map to their own conflict error.
    fn put_if_version(&mut self, task: Task, expected: i64) -> std::io::Result<()>;
    fn append_audit(&mut self, entry: AuditEntry) -> std::io::Result<()>;
    /// Allocate the next task id. On the trait because generating a recurring task's next
    /// occurrence is domain logic that has to mint an id without knowing the storage kind.
    ///
    /// Fallible because the alternative was worse: returning a bare `String` left the only
    /// implementation that can fail with nothing to do but invent one, and the caller then wrote
    /// a task under that invented id as though nothing had gone wrong.
    fn allocate_id(&mut self) -> std::io::Result<String>;
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
    /// Set by [`MemoryStore::let_another_writer_win_once`]; consumed by the next
    /// [`TaskStore::put_if_version`].
    interloper_pending: bool,
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

    /// Let another writer bump the stored version once, just before the next conditional write.
    ///
    /// A test seam for the lost update, which is otherwise unreachable in a single-threaded test:
    /// the window it exploits is between a caller reading a task and writing it back, and nothing
    /// in-process can land there. This puts a writer in that window exactly once.
    pub fn let_another_writer_win_once(&mut self) {
        self.interloper_pending = true;
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

    fn put_if_version(&mut self, task: Task, expected: i64) -> std::io::Result<()> {
        // The simulated other writer arrives here: after the caller read the task and decided what
        // to write, and before this check. That is precisely the window a real one exploits.
        if self.interloper_pending {
            self.interloper_pending = false;
            if let Some(t) = self.tasks.get_mut(&task.id) {
                t.version += 1;
            }
        }
        match self.tasks.get(&task.id).map(|t| t.version) {
            Some(v) if v == expected => self.put(task),
            other => Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!(
                    "{} is at version {}, not {expected}",
                    task.id,
                    other.map_or_else(|| "absent".to_string(), |v| v.to_string())
                ),
            )),
        }
    }

    fn append_audit(&mut self, entry: AuditEntry) -> std::io::Result<()> {
        if self.audit_fails {
            return Err(Self::refuse());
        }
        self.audit.push(entry);
        Ok(())
    }

    fn allocate_id(&mut self) -> std::io::Result<String> {
        self.next += 1;
        Ok(format!("TASK-{:04}", self.next + 1000))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Task;

    fn t(id: &str, version: i64) -> Task {
        let mut task = Task::new(id, "title", "main", "agent", "2026-09-01T00:00:00Z");
        task.version = version;
        task
    }

    #[test]
    fn a_write_is_refused_when_the_stored_version_moved_under_it() {
        // The lost update: two callers read version 1, both compute 2, and the later write wins
        // while reporting success. The earlier caller's change is gone with nothing said.
        let mut s = MemoryStore::new();
        s.put(t("TASK-0001", 1)).unwrap();

        // Another writer lands first.
        let mut theirs = t("TASK-0001", 2);
        theirs.title = "theirs".into();
        s.put(theirs).unwrap();

        // Ours was computed from version 1, so it must be refused rather than overwrite them.
        let mut mine = t("TASK-0001", 2);
        mine.title = "mine".into();
        let err = s
            .put_if_version(mine, 1)
            .expect_err("a stale write must be refused");
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists, "{err}");
        assert_eq!(
            s.get("TASK-0001").unwrap().title,
            "theirs",
            "and the other writer's change must survive"
        );
    }

    #[test]
    fn a_write_whose_expectation_still_holds_is_written() {
        let mut s = MemoryStore::new();
        s.put(t("TASK-0001", 1)).unwrap();
        let mut mine = t("TASK-0001", 2);
        mine.title = "mine".into();
        s.put_if_version(mine, 1)
            .expect("the uncontended write proceeds");
        assert_eq!(s.get("TASK-0001").unwrap().title, "mine");
    }

    #[test]
    fn a_write_expecting_a_task_that_is_not_there_is_refused() {
        let mut s = MemoryStore::new();
        assert!(
            s.put_if_version(t("TASK-0001", 2), 1).is_err(),
            "there is nothing to compare against, so this cannot be a safe write"
        );
    }
}
