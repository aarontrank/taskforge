use crate::model::Task;
use crate::store::{AuditEntry, TaskStore};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

/// Filesystem-backed store: one folder per task, plain text throughout.
///
/// Reads are fallible in principle but modelled as `Option` on the `TaskStore` trait, because
/// the domain layer only ever needs "is it there". Every write returns `io::Result` — task
/// writes, init, counter allocation, and log appends alike — so a caller can never mistake a
/// refused write for a successful one.
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
    ///
    /// **The id is claimed by creating its directory, not by the counter.** The counter is a
    /// read-modify-write with no lock, so two concurrent callers can read the same number; what
    /// stops them both using it is that `create_dir` fails atomically with `AlreadyExists`, so
    /// only one can take the id and the loser tries the next one. Without that, both callers were
    /// handed the same id and the second `put` silently overwrote the first task.
    ///
    /// **That uniqueness is per-workspace, because the claim is.** The counter lives at the root
    /// and is shared, while the claimed directory is under `workspaces/<name>/tasks/`, so two
    /// workspaces racing on the counter can each be handed the same id. That is deliberate rather
    /// than overlooked: a workspace is the unit of storage *and* of resolution — `get`, `list` and
    /// every id reference on a task resolve inside one — so the two tasks never meet, and neither
    /// can overwrite the other. The guarantee that matters is the one this provides: within a
    /// workspace, an id is never handed out twice. `two_workspaces_can_hold_the_same_id_and_stay_independent`
    /// pins the cross-workspace behaviour so it stays a known property rather than a surprise.
    pub fn next_id(&self) -> std::io::Result<String> {
        let path = self.root.join("task_counter.json");
        let tasks = self.workspace_dir().join("tasks");
        // Bounded so no filesystem state can spin here forever. The bound is far above any real
        // contention: reaching it means a thousand consecutive ids are already taken, which is a
        // repository to look at rather than a race to retry.
        //
        // Two paths below are deliberately untested: exhausting the bound (it needs a thousand
        // occupied ids to reach) and a `create_dir` failure that is not `AlreadyExists` (it needs
        // an unwritable tasks/ directory). Both are one-line propagations, and a test for the
        // second would turn on file permissions behaving identically for every user that builds
        // this — a flaky test on the build fleet in exchange for a covered `return Err(e)`.
        for _ in 0..1_000 {
            let next = Self::read_counter(&path)?;
            Self::write_counter(&path, next + 1)?;
            let id = format!("TASK-{next:04}");
            fs::create_dir_all(&tasks)?;
            match fs::create_dir(tasks.join(&id)) {
                Ok(()) => return Ok(id),
                // Someone else holds it. The counter has already moved on, so just try again.
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not find a free task id in 1000 attempts",
        ))
    }

    /// Replace the counter without ever leaving a partial file for a concurrent reader.
    ///
    /// `fs::write` truncates and then writes, so another process reading in that window sees an
    /// empty or half-written file. That was harmless only while a bad read silently became 1;
    /// now that it is correctly an error, a plain write turns ordinary concurrent creation into
    /// sporadic `IO_ERROR`s. Measured: 138 of 160 concurrent allocations failed that way.
    ///
    /// Writing a temp file and renaming it fixes that, because `rename` within a directory is
    /// atomic — a reader sees either the whole old file or the whole new one. The temp name
    /// carries both the pid and a per-process sequence, so no two writers can ever share one:
    /// if they did, one could rename away a file the other had only partly written, which is the
    /// very failure this avoids.
    fn write_counter(path: &std::path::Path, next: u64) -> std::io::Result<()> {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // Alongside the real file, so the rename cannot cross a filesystem boundary.
        let tmp = path.with_file_name(format!(
            "task_counter.json.{}.{unique}.tmp",
            std::process::id()
        ));
        fs::write(&tmp, format!("{{\"next\":{next}}}\n"))?;
        if let Err(e) = fs::rename(&tmp, path) {
            // Leaving the temp file behind would litter the repository root on every failure.
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        Ok(())
    }

    /// The next number the counter offers, or an error explaining why it cannot be read.
    ///
    /// Only **absence** legitimately means 1. Collapsing "unreadable" and "corrupt" into 1 as
    /// well — which is what a chain of `.ok()` did — restarts the sequence over a live
    /// repository, so the next create re-issues `TASK-0001` and overwrites it. Every other
    /// write path in this file propagates its IO errors for the same reason.
    fn read_counter(path: &std::path::Path) -> std::io::Result<u64> {
        match fs::read_to_string(path) {
            Ok(s) => serde_json::from_str::<serde_json::Value>(&s)
                .ok()
                .and_then(|v| v.get("next").and_then(|n| n.as_u64()))
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("{} has no readable `next` counter", path.display()),
                    )
                }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(1),
            Err(e) => Err(e),
        }
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

    /// Ids whose `task.md` exists but cannot be read back as a [`Task`].
    ///
    /// [`FsStore::get`] returns `None` for both "no such task" and "the file is there and
    /// unreadable", and [`FsStore::list`] filters `None` out — so a task file carrying a status
    /// this build does not recognize used to disappear from `list` and `show` with no diagnostic.
    /// Callers pair this with `list` to say so out loud.
    ///
    /// Requiring `task.md` to exist is what keeps a *reserved* id out of this list:
    /// [`FsStore::next_id`] claims an id by creating its directory, so an allocation whose write
    /// never happened leaves an empty folder that is not a corrupt task.
    pub fn unreadable_ids(&self) -> Vec<String> {
        self.list_ids()
            .into_iter()
            .filter(|id| self.task_dir(id).join("task.md").is_file() && self.get(id).is_none())
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
    ///
    /// Fallible because an empty-frontmatter fallback would be worse than an error: `get`
    /// reads a task with no frontmatter as absent, so a serialization failure would make the
    /// task disappear instead of reporting itself.
    fn render(task: &Task) -> std::io::Result<String> {
        let yaml = serde_yaml::to_string(task)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let description = task.description.clone().unwrap_or_default();
        // Summary / Acceptance Criteria / Notes, matching the template this replaced. The
        // middle heading is where a task records what "done" looks like, so a checklist has
        // somewhere to live without the writer inventing a structure.
        Ok(format!(
            "{FENCE}\n{yaml}{FENCE}\n\n## Summary\n\n{description}\n\n\
             ## Acceptance Criteria\n\n- [ ] \n\n## Notes\n\n"
        ))
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

    /// Write `task.md`, creating the task folder if it is new.
    ///
    /// Every failure is propagated. Swallowing them made a full disk or a read-only mount
    /// indistinguishable from a successful mutation, so the CLI reported a status and version
    /// that were never stored.
    fn put(&mut self, task: Task) -> std::io::Result<()> {
        let dir = self.task_dir(&task.id);
        for sub in ["attachments", "artifacts", "subtasks"] {
            fs::create_dir_all(dir.join(sub))?;
        }
        fs::write(dir.join("task.md"), Self::render(&task)?)
    }

    fn append_audit(&mut self, entry: AuditEntry) -> std::io::Result<()> {
        let line = serde_json::to_string(&entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        self.append_line(&entry.task_id.clone(), "audit.log", &line)
    }

    fn allocate_id(&mut self) -> std::io::Result<String> {
        self.next_id()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_missing_counter_legitimately_starts_at_one() {
        // The one case where starting over is correct: no counter file has been written yet.
        // Guards the fix below from over-reaching into this path.
        let d = tempfile::tempdir().unwrap();
        let s = FsStore::new(d.path(), "main");
        assert_eq!(s.next_id().expect("a fresh repository allocates"), "TASK-0001");
    }

    #[test]
    fn a_corrupt_counter_is_reported_rather_than_restarting_the_sequence() {
        // Restarting at 1 re-issues TASK-0001 and `put` overwrites the existing task — data loss
        // reported as success. Only "the file is not there yet" may legitimately mean 1.
        let (d, mut s) = root();
        s.put(task("TASK-0001")).unwrap();
        std::fs::write(d.path().join("task_counter.json"), "{ this is not json").unwrap();

        let err = s.next_id().expect_err("a corrupt counter must not silently reset");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData, "{err}");
        assert!(
            s.get("TASK-0001").is_some(),
            "the existing task must still be there"
        );
    }

    #[test]
    fn a_counter_missing_its_next_key_is_also_reported() {
        // Valid JSON, wrong shape — the other way the old `.ok()` chain collapsed into 1.
        let (d, s) = root();
        std::fs::write(d.path().join("task_counter.json"), "{\"nxet\":7}").unwrap();
        assert_eq!(
            s.next_id().expect_err("a counter with no usable next must be reported").kind(),
            std::io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn an_id_is_never_handed_out_twice_even_if_the_counter_is_stale() {
        // Stands in for two concurrent creates reading the same counter: rewind it and allocate
        // again. Without an atomic claim both callers get the same id and the second `put`
        // silently overwrites the first task.
        let (d, s) = root();
        let first = s.next_id().unwrap();
        std::fs::write(d.path().join("task_counter.json"), "{\"next\":1}").unwrap();
        let second = s.next_id().unwrap();
        assert_ne!(first, second, "two allocations must not collide");
    }

    #[test]
    fn a_task_whose_file_cannot_be_parsed_is_named_rather_than_dropped() {
        // `get` maps a parse failure to None and `list` filters None out, so a task file this
        // build cannot read disappears from `list` and `show` with no diagnostic at all.
        let (d, mut s) = root();
        s.put(task("TASK-0001")).unwrap();
        s.put(task("TASK-0002")).unwrap();
        let broken = d.path().join("workspaces/main/tasks/TASK-0002/task.md");
        std::fs::write(&broken, "---\nstatus: teleported\nid: TASK-0002\n---\n\n").unwrap();

        assert_eq!(s.list().len(), 1, "the readable task still lists");
        assert_eq!(
            s.unreadable_ids(),
            vec!["TASK-0002".to_string()],
            "the unreadable one must be reportable, not invisible"
        );
    }

    #[test]
    fn many_threads_allocating_at_once_all_succeed_with_distinct_ids() {
        // The real contention this store sees, exercised rather than reasoned about. It catches
        // both ways concurrent allocation can go wrong: two callers receiving the same id, and a
        // caller failing outright because it read the counter file mid-rewrite.
        let d = tempfile::tempdir().unwrap();
        let s = std::sync::Arc::new(FsStore::new(d.path(), "main"));
        s.init().unwrap();

        let threads: Vec<_> = (0..8)
            .map(|_| {
                let s = std::sync::Arc::clone(&s);
                std::thread::spawn(move || {
                    (0..20)
                        .map(|_| s.next_id())
                        .collect::<Vec<std::io::Result<String>>>()
                })
            })
            .collect();

        let mut ids = std::collections::BTreeSet::new();
        let mut failures = Vec::new();
        let mut total = 0;
        for t in threads {
            for r in t.join().expect("thread does not panic") {
                total += 1;
                match r {
                    Ok(id) => {
                        ids.insert(id);
                    }
                    Err(e) => failures.push(e.to_string()),
                }
            }
        }
        assert_eq!(total, 160, "every allocation is accounted for");
        assert!(
            failures.is_empty(),
            "concurrent allocation must not fail: {failures:?}"
        );
        assert_eq!(ids.len(), 160, "and every id must be distinct");
    }

    #[test]
    fn two_workspaces_can_hold_the_same_id_and_stay_independent() {
        // The atomic claim is the task *directory*, which is per-workspace, while the counter is
        // shared across the whole root. So the uniqueness guarantee is per-workspace, and two
        // workspaces racing on the counter can be handed the same id. Pinned by a test rather
        // than left as an unstated risk: it is benign because every read, write and reference
        // resolves inside one workspace, so the two tasks never meet.
        let d = tempfile::tempdir().unwrap();
        let mut main = FsStore::new(d.path(), "main");
        let mut side = FsStore::new(d.path(), "side");
        main.init().unwrap();
        side.init().unwrap();

        let a = main.next_id().unwrap();
        // Rewind the shared counter, standing in for the other workspace reading it first.
        std::fs::write(d.path().join("task_counter.json"), "{\"next\":1}").unwrap();
        let b = side.next_id().unwrap();
        assert_eq!(a, b, "the same id can be issued in two workspaces");

        let mut in_main = task(&a);
        in_main.workspace = "main".into();
        in_main.title = "the main one".into();
        let mut in_side = task(&b);
        in_side.workspace = "side".into();
        in_side.title = "the side one".into();
        main.put(in_main).unwrap();
        side.put(in_side).unwrap();

        assert_eq!(main.get(&a).unwrap().title, "the main one");
        assert_eq!(side.get(&b).unwrap().title, "the side one", "neither overwrote the other");
    }

    #[test]
    fn a_reserved_id_with_no_task_file_is_not_reported_as_unreadable() {
        // `next_id` claims an id by creating its folder, so a create that never got as far as
        // writing task.md leaves an empty one. That is an unused reservation, not a corrupt task,
        // and reporting it would cry wolf on every abandoned allocation.
        let (d, s) = root();
        let id = s.next_id().unwrap();
        assert!(
            d.path().join("workspaces/main/tasks").join(&id).is_dir(),
            "the id is claimed by its directory"
        );
        assert!(
            s.unreadable_ids().is_empty(),
            "an empty reservation is not an unreadable task: {:?}",
            s.unreadable_ids()
        );
    }

    #[test]
    fn an_unreadable_counter_is_propagated_rather_than_read_as_absent() {
        // Only NotFound may mean "start at 1". A counter that exists but cannot be read is a
        // different problem and must surface as itself — here a directory in its place, which
        // read_to_string refuses with something other than NotFound.
        let (d, s) = root();
        let counter = d.path().join("task_counter.json");
        std::fs::remove_file(&counter).unwrap();
        std::fs::create_dir(&counter).unwrap();

        let err = s.next_id().expect_err("an unreadable counter must be reported");
        assert_ne!(
            err.kind(),
            std::io::ErrorKind::NotFound,
            "and not mistaken for a missing one: {err}"
        );
    }

    #[test]
    fn allocate_id_reports_a_failure_instead_of_inventing_an_id() {
        // The trait used to return String and swallowed the error into `TASK-ERR-<pid>`, which a
        // caller would then happily write to disk as a real task.
        let (d, mut s) = root();
        std::fs::write(d.path().join("task_counter.json"), "nonsense").unwrap();
        assert!(s.allocate_id().is_err(), "a failed allocation must be an error");
    }

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
        t.reviews = vec!["PR-1".into()];
        t.expected_by = Some("2026-09-03T00:00:00Z".into());
        t.worker = Some("w1".into());
        t.blocked_by = vec!["TASK-0002".into()];
        s.put(t.clone()).unwrap();

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
        s.put(task("TASK-0001")).unwrap();
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
        s.put(t).unwrap();
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
        s.put(task("TASK-0001")).unwrap();
        for n in 1..=2 {
            s.append_audit(AuditEntry {
                ts: format!("2026-09-0{n}T00:00:00Z"),
                actor: "aaron".into(),
                action: "status_changed".into(),
                task_id: "TASK-0001".into(),
                from: None,
                to: Some("running".into()),
            })
            .unwrap();
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
        s.put(task("TASK-0001")).unwrap();
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

    #[test]
    fn a_task_file_with_no_frontmatter_fence_reads_as_none() {
        let (d, mut s) = root();
        s.put(task("TASK-0001")).unwrap();
        let f = d.path().join("workspaces/main/tasks/TASK-0001/task.md");
        std::fs::write(&f, "just a body, no fence\n").unwrap();
        assert!(
            s.get("TASK-0001").is_none(),
            "a malformed file is absent, not a panic"
        );
    }

    #[test]
    fn a_task_file_with_unparseable_frontmatter_reads_as_none() {
        let (d, mut s) = root();
        s.put(task("TASK-0001")).unwrap();
        let f = d.path().join("workspaces/main/tasks/TASK-0001/task.md");
        std::fs::write(&f, "---\nid: [unclosed\n---\n\nbody\n").unwrap();
        assert!(s.get("TASK-0001").is_none());
    }

    #[test]
    fn attaching_a_missing_source_file_is_an_error() {
        let (_d, mut s) = root();
        s.put(task("TASK-0001")).unwrap();
        let err = s
            .attach(
                "TASK-0001",
                "attachments",
                std::path::Path::new("/nope/x.txt"),
                true,
            )
            .unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    /// A write that the filesystem refuses must surface as an error, not vanish.
    ///
    /// Unix-only because it works by removing write permission; there is no portable way to
    /// simulate a full disk, and this is the failure mode that actually reaches users.
    #[cfg(unix)]
    #[test]
    fn a_write_to_an_unwritable_task_file_is_an_error() {
        use std::os::unix::fs::PermissionsExt;
        let (_d, mut s) = root();
        s.put(task("TASK-0001")).expect("the first write succeeds");

        let f = s.task_dir("TASK-0001").join("task.md");
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o444)).unwrap();

        let mut t = task("TASK-0001");
        t.status = TaskStatus::Running;
        let err = s.put(t).expect_err("a refused write is an error");
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);

        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            s.get("TASK-0001").unwrap().status,
            TaskStatus::Open,
            "and the stored task is unchanged"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_refused_audit_append_is_an_error() {
        use std::os::unix::fs::PermissionsExt;
        let (_d, mut s) = root();
        s.put(task("TASK-0001")).unwrap();
        let dir = s.task_dir("TASK-0001");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();

        let err = s
            .append_audit(AuditEntry {
                ts: "2026-09-01T00:00:00Z".into(),
                actor: "aaron".into(),
                action: "status_changed".into(),
                task_id: "TASK-0001".into(),
                from: None,
                to: Some("running".into()),
            })
            .expect_err("an audit append that cannot happen is an error");
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn the_task_body_keeps_a_place_for_acceptance_criteria() {
        // The pre-Rust template had Summary / Acceptance Criteria / Notes. Dropping the middle
        // heading in the port lost the one place a task says what "done" would look like, which
        // is a first-class section in an orchestrate per-stream file.
        let (d, mut s) = root();
        s.put(task("TASK-0001")).unwrap();
        let text =
            std::fs::read_to_string(d.path().join("workspaces/main/tasks/TASK-0001/task.md"))
                .unwrap();
        let headings: Vec<&str> = text.lines().filter(|l| l.starts_with("## ")).collect();
        assert_eq!(
            headings,
            vec!["## Summary", "## Acceptance Criteria", "## Notes"]
        );
    }
}
