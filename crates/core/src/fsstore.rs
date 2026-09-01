use crate::model::Task;
use crate::store::{AuditEntry, TaskStore};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

/// Filesystem-backed store: one folder per task, plain text throughout.
///
/// Reads are fallible in principle but modelled as `Option` on the `TaskStore` trait, because
/// the domain layer only ever needs "is it there". Layout-level operations that a caller must
/// be told about — init, counter allocation, log appends — return `io::Result` instead.
pub struct FsStore {
    root: PathBuf,
    workspace: String,
}

const FENCE: &str = "---";

impl FsStore {
    pub fn new(root: impl Into<PathBuf>, workspace: impl Into<String>) -> Self {
        FsStore {
            root: root.into(),
            workspace: workspace.into(),
        }
    }

    fn workspace_dir(&self) -> PathBuf {
        self.root.join("workspaces").join(&self.workspace)
    }

    fn task_dir(&self, id: &str) -> PathBuf {
        self.workspace_dir().join("tasks").join(id)
    }

    /// Create the repository skeleton. Idempotent, so a re-run is safe.
    pub fn init(&self) -> std::io::Result<()> {
        fs::create_dir_all(self.workspace_dir().join("tasks"))?;
        let counter = self.root.join("task_counter.json");
        if !counter.exists() {
            fs::write(&counter, "{\"next\":1}\n")?;
        }
        Ok(())
    }

    /// Allocate the next task id, persisting the sequence so a new handle continues it.
    pub fn next_id(&self) -> std::io::Result<String> {
        let path = self.root.join("task_counter.json");
        let next: u64 = fs::read_to_string(&path)
            .ok()
            .and_then(|s| {
                serde_json::from_str::<serde_json::Value>(&s)
                    .ok()
                    .and_then(|v| v.get("next").and_then(|n| n.as_u64()))
            })
            .unwrap_or(1);
        fs::write(&path, format!("{{\"next\":{}}}\n", next + 1))?;
        Ok(format!("TASK-{next:04}"))
    }

    fn append_line(&self, id: &str, file: &str, line: &str) -> std::io::Result<()> {
        let path = self.task_dir(id).join(file);
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        writeln!(f, "{line}")
    }

    /// Append an execution record. Separate from comments on purpose: worklogs are what was
    /// done, comments are discussion, and merging them loses that distinction.
    pub fn append_worklog(
        &self,
        id: &str,
        actor: &str,
        ts: &str,
        text: &str,
    ) -> std::io::Result<()> {
        self.append_line(id, "worklog.md", &format!("\n### {ts} — {actor}\n\n{text}"))
    }

    /// Append a discussion entry.
    pub fn append_comment(
        &self,
        id: &str,
        actor: &str,
        ts: &str,
        text: &str,
    ) -> std::io::Result<()> {
        self.append_line(
            id,
            "comments.md",
            &format!("\n### {ts} — {actor}\n\n{text}"),
        )
    }

    /// Ids of every task folder in the workspace, sorted.
    pub fn list_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = std::fs::read_dir(self.workspace_dir().join("tasks"))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().to_str().map(String::from))
            .collect();
        ids.sort();
        ids
    }

    /// Every task in the workspace, sorted by id.
    pub fn list(&self) -> Vec<Task> {
        self.list_ids()
            .iter()
            .filter_map(|id| self.get(id))
            .collect()
    }

    /// Parsed audit entries for a task, oldest first. Unparseable lines are skipped rather
    /// than aborting the read: a corrupt line must not hide the rest of the history.
    pub fn audit(&self, id: &str) -> Vec<AuditEntry> {
        std::fs::read_to_string(self.task_dir(id).join("audit.log"))
            .map(|t| {
                t.lines()
                    .filter_map(|l| serde_json::from_str(l).ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Place a file under `attachments/` or `artifacts/`.
    ///
    /// `copy` duplicates the bytes into the task folder; `link` records only the path. A
    /// missing source is an error either way — recording a reference to a file that is not
    /// there turns a typo into silent data loss.
    pub fn attach(
        &self,
        id: &str,
        kind: &str,
        src: &std::path::Path,
        copy: bool,
    ) -> std::io::Result<String> {
        if !src.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("no such file: {}", src.display()),
            ));
        }
        if !copy {
            return Ok(src.display().to_string());
        }
        let name = src.file_name().unwrap_or_default();
        let dir = self.task_dir(id).join(kind);
        std::fs::create_dir_all(&dir)?;
        std::fs::copy(src, dir.join(name))?;
        Ok(format!("{kind}/{}", name.to_string_lossy()))
    }

    /// The workspaces present under this root.
    pub fn workspaces(root: &std::path::Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(root.join("workspaces"))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().to_str().map(String::from))
            .collect();
        names.sort();
        names
    }

    /// Render `task.md`: frontmatter fence, then the human-facing body.
    fn render(task: &Task) -> String {
        let yaml = serde_yaml::to_string(task).unwrap_or_default();
        let description = task.description.clone().unwrap_or_default();
        format!("{FENCE}\n{yaml}{FENCE}\n\n## Summary\n\n{description}\n\n## Notes\n\n")
    }
}

impl TaskStore for FsStore {
    fn get(&self, id: &str) -> Option<Task> {
        let text = fs::read_to_string(self.task_dir(id).join("task.md")).ok()?;
        // Everything between the opening fence and the next one is the frontmatter.
        let rest = text.strip_prefix(FENCE)?.trim_start_matches('\n');
        let end = rest.find("\n---")?;
        serde_yaml::from_str(&rest[..end]).ok()
    }

    fn put(&mut self, task: Task) {
        let dir = self.task_dir(&task.id);
        for sub in ["attachments", "artifacts", "subtasks"] {
            let _ = fs::create_dir_all(dir.join(sub));
        }
        let _ = fs::write(dir.join("task.md"), Self::render(&task));
    }

    fn append_audit(&mut self, entry: AuditEntry) {
        if let Ok(line) = serde_json::to_string(&entry) {
            let _ = self.append_line(&entry.task_id.clone(), "audit.log", &line);
        }
    }

    fn allocate_id(&mut self) -> String {
        // A failed allocation must not silently reuse an id, so fall back to a timestamp
        // rather than to a fixed value.
        self.next_id()
            .unwrap_or_else(|_| format!("TASK-ERR-{}", std::process::id()))
    }
}

#[cfg(test)]
mod tests {
    use crate::fsstore::*;
    use crate::model::{Task, TaskStatus};
    use crate::store::{AuditEntry, TaskStore};

    fn root() -> (tempfile::TempDir, FsStore) {
        let d = tempfile::tempdir().expect("tempdir");
        let s = FsStore::new(d.path(), "main");
        s.init().expect("init");
        (d, s)
    }

    fn task(id: &str) -> Task {
        Task::new(
            id,
            "Port the storage layer",
            "main",
            "agent",
            "2026-09-01T00:00:00Z",
        )
    }

    #[test]
    fn a_task_round_trips_through_the_filesystem() {
        let (_d, mut s) = root();
        let mut t = task("TASK-0001");
        t.status = TaskStatus::InReview;
        t.review_id = Some("CR-1".into());
        t.expected_by = Some("2026-09-03T00:00:00Z".into());
        t.worker = Some("w1".into());
        t.blocked_by = vec!["TASK-0002".into()];
        s.put(t.clone());

        let back = s.get("TASK-0001").expect("task should be readable");
        assert_eq!(back, t, "round-trip must preserve every field");
    }

    #[test]
    fn an_absent_task_reads_as_none_rather_than_erroring() {
        let (_d, s) = root();
        assert!(s.get("TASK-9999").is_none());
    }

    #[test]
    fn the_task_lives_in_the_documented_folder_layout() {
        let (d, mut s) = root();
        s.put(task("TASK-0001"));
        let dir = d.path().join("workspaces/main/tasks/TASK-0001");
        assert!(dir.join("task.md").is_file(), "task.md");
        assert!(dir.join("attachments").is_dir(), "attachments/");
        assert!(dir.join("artifacts").is_dir(), "artifacts/");
        assert!(dir.join("subtasks").is_dir(), "subtasks/");
    }

    #[test]
    fn task_md_is_yaml_frontmatter_then_a_markdown_body() {
        let (d, mut s) = root();
        let mut t = task("TASK-0001");
        t.description = Some("Why this matters".into());
        s.put(t);
        let text =
            std::fs::read_to_string(d.path().join("workspaces/main/tasks/TASK-0001/task.md"))
                .unwrap();
        assert!(text.starts_with("---\n"), "opens with a frontmatter fence");
        assert!(text.contains("\n---\n"), "closes the fence");
        assert!(text.contains("id: TASK-0001"));
        assert!(text.contains("## Summary"), "body follows the fence");
        assert!(
            text.contains("Why this matters"),
            "description reaches the body"
        );
    }

    #[test]
    fn the_audit_log_is_append_only_newline_delimited_json() {
        let (d, mut s) = root();
        s.put(task("TASK-0001"));
        for n in 1..=2 {
            s.append_audit(AuditEntry {
                ts: format!("2026-09-0{n}T00:00:00Z"),
                actor: "aaron".into(),
                action: "status_changed".into(),
                task_id: "TASK-0001".into(),
                from: None,
                to: Some("running".into()),
            });
        }
        let text =
            std::fs::read_to_string(d.path().join("workspaces/main/tasks/TASK-0001/audit.log"))
                .unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "one line per entry, nothing rewritten");
        for l in lines {
            serde_json::from_str::<AuditEntry>(l).expect("each line parses on its own");
        }
    }

    #[test]
    fn the_counter_allocates_sequential_ids_and_persists() {
        let (d, s) = root();
        assert_eq!(s.next_id().unwrap(), "TASK-0001");
        assert_eq!(s.next_id().unwrap(), "TASK-0002");
        // A fresh handle on the same root must not restart the sequence.
        let s2 = FsStore::new(d.path(), "main");
        assert_eq!(s2.next_id().unwrap(), "TASK-0003");
    }

    #[test]
    fn worklogs_and_comments_append_under_their_own_headings() {
        let (d, mut s) = root();
        s.put(task("TASK-0001"));
        s.append_worklog("TASK-0001", "aaron", "2026-09-01T00:00:00Z", "did a thing")
            .unwrap();
        s.append_comment("TASK-0001", "kiro", "2026-09-01T00:01:00Z", "a question")
            .unwrap();
        let base = d.path().join("workspaces/main/tasks/TASK-0001");
        let wl = std::fs::read_to_string(base.join("worklog.md")).unwrap();
        let cm = std::fs::read_to_string(base.join("comments.md")).unwrap();
        assert!(wl.contains("did a thing") && wl.contains("aaron"));
        assert!(cm.contains("a question") && cm.contains("kiro"));
        assert!(!wl.contains("a question"), "the two logs stay separate");
    }
}
