//! The on-disk store: one directory per task, plain text throughout.
//!
//! The format is the feature. A task is a `task.md` whose YAML frontmatter is the [`Task`],
//! followed by a markdown body, so the record stays readable with an editor, `grep` and `git`
//! when this binary is unavailable or wrong.
//!
//! **Editable, with one section reserved.** The frontmatter is the [`Task`] and round-trips.
//! `## Summary` is generated from the `description` field, so editing it there is pointless — it is
//! rewritten on the next mutation, which is what keeps it from disagreeing with the field. From the
//! first heading after it to the end of the file, the body is the writer's and is carried through
//! every mutation byte-for-byte.
//!
//! Replacement is atomic — a temp file and a rename — so a concurrent reader never sees a task
//! mid-write. `write_atomically` records what that measured before it was fixed; it is deliberately
//! named without a doc link, because it is private and linking to it from a public module doc leaves
//! a dead reference in the generated documentation.

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

/// The body a new task starts with, after the generated `## Summary`.
///
/// A checklist heading so "what does done mean" has somewhere to live without the writer having to
/// invent a structure, and a notes heading for everything else.
const DEFAULT_TAIL: &str = "## Acceptance Criteria\n\n- [ ] \n\n## Notes\n\n";

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

    /// Replace a file's contents without ever exposing a partial state to a reader.
    ///
    /// `fs::write` truncates and then fills, so a concurrent reader lands in a window where the
    /// file is empty or half written. Both places this matters were measured, and neither is
    /// theoretical:
    ///
    /// * the counter — 138 of 160 concurrent allocations failed once an unreadable counter
    ///   correctly became an error rather than silently restarting at 1; and
    /// * a task file — **7368 of 7983** reads saw the task as *absent* while it was being
    ///   rewritten, because a `task.md` with no frontmatter yet is indistinguishable from a task
    ///   that does not exist.
    ///
    /// Writing a temp file and renaming it removes the window: `rename` within a directory is
    /// atomic, so a reader sees the whole old file or the whole new one. The temp name carries the
    /// pid and a per-process sequence, so no two writers can share one — if they did, one could
    /// rename away a file the other had only partly written, which is the failure being avoided.
    fn write_atomically(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
        let staged = Self::stage(path, contents)?;
        Self::commit(&staged, path)
    }

    /// Write the new contents to a temp file beside `path`, ready to be renamed into place.
    ///
    /// Split from [`FsStore::commit`] so a caller can do work — specifically the
    /// optimistic-concurrency re-check in [`FsStore::put_if_version`] — after the expensive part
    /// and immediately before the single syscall that publishes it.
    fn stage(path: &std::path::Path, contents: &str) -> std::io::Result<PathBuf> {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        // Alongside the real file, so the rename cannot cross a filesystem boundary. Dot-prefixed
        // so a temp left by a killed process does not look like content.
        let tmp = path.with_file_name(format!(".{name}.{}.{unique}.tmp", std::process::id()));
        fs::write(&tmp, contents)?;
        Ok(tmp)
    }

    /// Publish staged contents, or clean up if the rename fails.
    fn commit(staged: &std::path::Path, path: &std::path::Path) -> std::io::Result<()> {
        if let Err(e) = fs::rename(staged, path) {
            // Leaving the temp behind would litter the repository on every failure.
            let _ = fs::remove_file(staged);
            return Err(e);
        }
        Ok(())
    }

    fn write_counter(path: &std::path::Path, next: u64) -> std::io::Result<()> {
        Self::write_atomically(path, &format!("{{\"next\":{next}}}\n"))
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
    fn render(task: &Task, tail: Option<&str>) -> std::io::Result<String> {
        let yaml = serde_yaml::to_string(task)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let description = task.description.clone().unwrap_or_default();
        // `## Summary` is generated from `description`, so the two can never disagree. Everything
        // after it is the writer's: `tail` carries it forward, and `DEFAULT_TAIL` seeds a new task
        // with somewhere to record what "done" looks like.
        let tail = tail.unwrap_or(DEFAULT_TAIL);
        Ok(format!(
            "{FENCE}\n{yaml}{FENCE}\n\n## Summary\n\n{description}\n\n{tail}"
        ))
    }

    /// The part of an existing `task.md` that belongs to whoever wrote it.
    ///
    /// Everything from the first `## ` heading after the generated `## Summary` to the end of the
    /// file, returned as a slice of `text` so it is carried forward byte-for-byte. `None` when
    /// there is no such heading — a brand-new task, or a body holding nothing but a summary — and
    /// the caller then uses [`DEFAULT_TAIL`].
    ///
    /// Keyed on "the first heading that is not Summary" rather than on the template's own headings,
    /// because nothing obliges a writer to keep them: a body reorganised under `## Design sketch`
    /// must survive just as `## Notes` does.
    fn preserved_tail(text: &str) -> Option<&str> {
        let (front, body) = Self::split_frontmatter(text)?;
        let body = body.trim_start_matches('\n');
        // Strip exactly the Summary section this store would have written, using the description
        // recorded in the file's *own* frontmatter rather than the one being written now. Whatever
        // remains is the tail.
        //
        // Scanning for "the first heading that is not `## Summary`" instead is wrong, and wrong in a
        // way that corrupts: `description` is free-form prose rendered *inside* the Summary section,
        // so a description containing `## Risks` made that heading look like the start of the
        // writer's body. `render` then re-emitted the description and appended a tail beginning
        // mid-description — measured at three copies after three writes.
        let stored: Task = serde_yaml::from_str(front).ok()?;
        let described = stored.description.unwrap_or_default();
        let summary = format!("## Summary\n\n{described}\n\n");
        let tail = match body.strip_prefix(&summary) {
            Some(tail) => tail,
            // The Summary section is not what this store writes — a hand-rewritten body. There is
            // no exact boundary left to match, so fall back to the first heading that did not come
            // from the description.
            None => Self::first_heading_not_in(body, &described)?,
        };
        (!tail.is_empty()).then_some(tail)
    }

    /// Split `text` into its frontmatter and everything after the closing fence.
    fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
        let rest = text.strip_prefix(FENCE)?.trim_start_matches('\n');
        let end = rest.find("\n---")?;
        Some((&rest[..end], &rest[end + "\n---".len()..]))
    }

    /// Fallback tail boundary: the first `## ` heading that neither is `## Summary` nor appears in
    /// `described`.
    ///
    /// Only reached when the Summary section has been hand-rewritten, so its exact extent cannot be
    /// matched. Skipping headings that occur in the description is what stops this re-introducing the
    /// duplication bug: a description containing `## Risks` renders that line inside the Summary
    /// section, and treating it as the tail's start meant the next write emitted the description
    /// again *and* appended a tail beginning mid-description — growing the file on every mutation.
    ///
    /// The residual case, accepted knowingly: a writer whose own tail heading is *also* one of the
    /// description's headings loses that one section from the tail. That is a bounded, one-time loss
    /// against unbounded duplication on every write, and it needs a hand-edited Summary to reach at
    /// all. A `## ` boundary is only ambiguous because the generated section may contain arbitrary
    /// markdown; an explicit end-of-Summary marker in the file would remove the ambiguity, at the
    /// cost of a format change and a migration for every existing task.
    fn first_heading_not_in<'b>(body: &'b str, described: &str) -> Option<&'b str> {
        let mut offset = 0usize;
        for line in body.split_inclusive('\n') {
            let heading = line.trim_end();
            if heading.starts_with("## ")
                && heading != "## Summary"
                && !described.lines().any(|l| l.trim_end() == heading)
            {
                return Some(&body[offset..]);
            }
            offset += line.len();
        }
        None
    }
}

impl TaskStore for FsStore {
    fn get(&self, id: &str) -> Option<Task> {
        let text = fs::read_to_string(self.task_dir(id).join("task.md")).ok()?;
        let (front, _) = Self::split_frontmatter(&text)?;
        serde_yaml::from_str(front).ok()
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
        let path = dir.join("task.md");
        // Read the file back so the writer's own body survives the rewrite. An unreadable or
        // absent file simply yields the default tail — a task that cannot be read is not a reason
        // to refuse to write one.
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let rendered = Self::render(&task, Self::preserved_tail(&existing))?;
        Self::write_atomically(&path, &rendered)
    }

    fn put_if_version(&mut self, task: Task, expected: i64) -> std::io::Result<()> {
        let dir = self.task_dir(&task.id);
        for sub in ["attachments", "artifacts", "subtasks"] {
            fs::create_dir_all(dir.join(sub))?;
        }
        let path = dir.join("task.md");
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let rendered = Self::render(&task, Self::preserved_tail(&existing))?;
        // Stage first, then check, then publish. The check goes last on purpose: reading the file,
        // rendering and writing the temp are all slow next to a `rename`, so doing them before the
        // comparison leaves the smallest possible window in which another writer can land.
        let staged = Self::stage(&path, &rendered)?;
        match self.get(&task.id).map(|t| t.version) {
            Some(v) if v == expected => Self::commit(&staged, &path),
            other => {
                let _ = fs::remove_file(&staged);
                Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!(
                        "{} is at version {}, not {expected}",
                        task.id,
                        other.map_or_else(|| "absent".to_string(), |v| v.to_string())
                    ),
                ))
            }
        }
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
    fn a_mutation_preserves_the_hand_written_body() {
        // `put` re-rendered the whole file from the Task, so every mutation replaced the body with
        // a blank template. The docs invite a human to edit these files; this is what makes that
        // true. Confirmed by experiment before the fix: an edited checklist and note were gone
        // after a single `task start`.
        let (_d, mut s) = root();
        let mut t = task("TASK-0001");
        t.description = Some("Port the storage layer".into());
        s.put(t.clone()).unwrap();

        // Stand in for a human editing the file.
        let path = s.task_dir("TASK-0001").join("task.md");
        let edited = std::fs::read_to_string(&path)
            .unwrap()
            .replace(
                "- [ ] \n",
                "- [x] fsstore round-trips\n- [ ] audit log appends\n",
            )
            .replace(
                "## Notes\n\n",
                "## Notes\n\nTalked to Dana; see the sketch.\n",
            );
        std::fs::write(&path, &edited).unwrap();

        // Any later mutation.
        t.status = TaskStatus::InReview;
        t.version = 2;
        s.put(t).unwrap();

        let after = std::fs::read_to_string(&path).unwrap();
        assert!(
            after.contains("- [x] fsstore round-trips"),
            "criteria kept:\n{after}"
        );
        assert!(
            after.contains("- [ ] audit log appends"),
            "and the added line:\n{after}"
        );
        assert!(
            after.contains("Talked to Dana; see the sketch."),
            "notes kept:\n{after}"
        );
        assert!(
            after.contains("status: in-review"),
            "and the frontmatter still updated:\n{after}"
        );
    }

    #[test]
    fn a_description_containing_a_heading_does_not_get_duplicated() {
        // `description` is rendered inside the Summary section, and the tail scan looked for the
        // first `## ` heading that was not `## Summary` — so a heading *inside* the description was
        // taken to be where the writer's body began. `render` then emitted the description again
        // and appended a tail starting mid-description, duplicating content on every write.
        let (_d, mut s) = root();
        let mut t = task("TASK-0001");
        t.description = Some("Do the thing.\n\n## Risks\n\nIt might not work.".into());
        s.put(t.clone()).unwrap();

        t.version = 2;
        s.put(t.clone()).unwrap();
        t.version = 3;
        s.put(t).unwrap();

        let after = std::fs::read_to_string(s.task_dir("TASK-0001").join("task.md")).unwrap();
        let body = after.split_once("\n---\n").unwrap().1;
        assert_eq!(
            body.matches("## Risks").count(),
            1,
            "the description's own heading must appear once, not once per write:\n{after}"
        );
        assert_eq!(
            body.matches("It might not work.").count(),
            1,
            "and neither must its text:\n{after}"
        );
        assert_eq!(
            body.matches("## Acceptance Criteria").count(),
            1,
            "and the real tail must still be there exactly once:\n{after}"
        );
    }

    #[test]
    fn a_hand_edited_summary_does_not_duplicate_a_heading_from_the_description() {
        // The exact-strip path cannot match once a human has edited the Summary text, so the
        // fallback runs — and the fallback looks for the first `## ` heading, which for a
        // description containing one is a line inside the description itself. That is the same
        // duplication bug, in exactly the case this feature exists for: humans editing these files.
        let (_d, mut s) = root();
        let mut t = task("TASK-0001");
        t.description = Some("Do it.\n\n## Risks\n\nMaybe not.".into());
        s.put(t.clone()).unwrap();

        // A human rewrites the summary prose, leaving everything else alone. Only the BODY: editing
        // the frontmatter too would keep the stored description in step with it, and the exact-strip
        // path would still match — which is not the situation being tested.
        let path = s.task_dir("TASK-0001").join("task.md");
        let text = std::fs::read_to_string(&path).unwrap();
        let (front, body) = text.split_once("\n---\n").unwrap();
        let edited = body.replace("Do it.\n", "Do it, but carefully.\n");
        assert_ne!(edited, body, "the edit must actually change the body");
        std::fs::write(&path, format!("{front}\n---\n{edited}")).unwrap();

        for v in 2..5 {
            t.version = v;
            s.put(t.clone()).unwrap();
        }

        let after = std::fs::read_to_string(&path).unwrap();
        let body = after.split_once("\n---\n").unwrap().1;
        assert_eq!(
            body.matches("## Risks").count(),
            1,
            "the description's heading must not accumulate:\n{after}"
        );
        assert_eq!(
            body.matches("Maybe not.").count(),
            1,
            "nor its text:\n{after}"
        );
        assert_eq!(
            body.matches("## Acceptance Criteria").count(),
            1,
            "and the writer's tail must survive exactly once:\n{after}"
        );
    }

    #[test]
    fn the_summary_section_still_follows_the_description() {
        // The counterpart to preserving the body: `## Summary` is generated, so it must not go
        // stale when the description changes. Preserving the *whole* body would do exactly that.
        let (_d, mut s) = root();
        let mut t = task("TASK-0001");
        t.description = Some("first wording".into());
        s.put(t.clone()).unwrap();

        t.description = Some("second wording".into());
        s.put(t).unwrap();

        let after = std::fs::read_to_string(s.task_dir("TASK-0001").join("task.md")).unwrap();
        assert!(
            after.contains("second wording"),
            "summary follows description:\n{after}"
        );
        assert!(
            !after.contains("first wording"),
            "and the old wording is gone:\n{after}"
        );
    }

    #[test]
    fn a_body_with_its_own_headings_is_preserved_whole() {
        // Nothing requires the body to use the template's headings. Whatever the first heading
        // after Summary is, everything from there down is the writer's and is kept.
        let (_d, mut s) = root();
        let mut t = task("TASK-0001");
        t.description = Some("desc".into());
        s.put(t.clone()).unwrap();
        let path = s.task_dir("TASK-0001").join("task.md");
        let text = std::fs::read_to_string(&path).unwrap();
        let (front, _) = text.split_once("## Acceptance Criteria").unwrap();
        std::fs::write(&path, format!("{front}## Design sketch\n\nA then B.\n")).unwrap();

        t.version = 2;
        s.put(t).unwrap();

        let after = std::fs::read_to_string(&path).unwrap();
        assert!(
            after.contains("## Design sketch"),
            "custom heading kept:\n{after}"
        );
        assert!(after.contains("A then B."), "and its content:\n{after}");
    }

    #[test]
    fn a_reader_never_sees_a_half_written_task() {
        // `put` wrote in place, so it truncated the file and then filled it. A concurrent reader
        // landing in that window read no frontmatter, which this store reports as *absent* — the
        // task blinks out of existence mid-write. Replacing the file by rename removes the window:
        // a reader sees the whole old file or the whole new one.
        let d = tempfile::tempdir().unwrap();
        let s = std::sync::Arc::new(FsStore::new(d.path(), "main"));
        s.init().unwrap();
        {
            let mut w = FsStore::new(d.path(), "main");
            w.put(task("TASK-0001")).unwrap();
        }

        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let reader = {
            let s = std::sync::Arc::clone(&s);
            let stop = std::sync::Arc::clone(&stop);
            std::thread::spawn(move || {
                let mut vanished = 0u32;
                let mut reads = 0u32;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    reads += 1;
                    if s.get("TASK-0001").is_none() {
                        vanished += 1;
                    }
                }
                (reads, vanished)
            })
        };

        let mut w = FsStore::new(d.path(), "main");
        for n in 0..400 {
            let mut t = task("TASK-0001");
            t.version = n;
            t.description = Some(format!("revision {n}"));
            w.put(t).unwrap();
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let (reads, vanished) = reader.join().expect("reader does not panic");

        assert!(reads > 0, "the reader must actually have read something");
        assert_eq!(
            vanished, 0,
            "a task must never read as absent while it is being written ({vanished} of {reads} reads)"
        );
    }

    #[test]
    fn a_conditional_write_is_refused_on_disk_when_the_version_moved() {
        // The filesystem half of the lost-update guard: it stages the new file, re-reads the stored
        // version, and only then renames. A refused write must leave the other writer's file and no
        // staged leftovers.
        let (_d, mut s) = root();
        s.put(task("TASK-0001")).unwrap();
        let mut theirs = task("TASK-0001");
        theirs.version = 2;
        theirs.title = "theirs".into();
        s.put(theirs).unwrap();

        let mut mine = task("TASK-0001");
        mine.version = 2;
        mine.title = "mine".into();
        let err = s
            .put_if_version(mine, 1)
            .expect_err("a write computed from version 1 must be refused");
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists, "{err}");
        assert_eq!(s.get("TASK-0001").unwrap().title, "theirs");
        let leftovers: Vec<_> = std::fs::read_dir(s.task_dir("TASK-0001"))
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "no staged file may be left: {leftovers:?}"
        );
    }

    #[test]
    fn a_missing_counter_legitimately_starts_at_one() {
        // The one case where starting over is correct: no counter file has been written yet.
        // Guards the fix below from over-reaching into this path.
        let d = tempfile::tempdir().unwrap();
        let s = FsStore::new(d.path(), "main");
        assert_eq!(
            s.next_id().expect("a fresh repository allocates"),
            "TASK-0001"
        );
    }

    #[test]
    fn a_corrupt_counter_is_reported_rather_than_restarting_the_sequence() {
        // Restarting at 1 re-issues TASK-0001 and `put` overwrites the existing task — data loss
        // reported as success. Only "the file is not there yet" may legitimately mean 1.
        let (d, mut s) = root();
        s.put(task("TASK-0001")).unwrap();
        std::fs::write(d.path().join("task_counter.json"), "{ this is not json").unwrap();

        let err = s
            .next_id()
            .expect_err("a corrupt counter must not silently reset");
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
            s.next_id()
                .expect_err("a counter with no usable next must be reported")
                .kind(),
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
        assert_eq!(
            side.get(&b).unwrap().title,
            "the side one",
            "neither overwrote the other"
        );
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

        let err = s
            .next_id()
            .expect_err("an unreadable counter must be reported");
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
        assert!(
            s.allocate_id().is_err(),
            "a failed allocation must be an error"
        );
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

        // The *directory*, not the file. `put` now writes a temp beside `task.md` and renames it,
        // and `rename` needs write permission on the directory rather than on the target — so
        // chmodding the file alone no longer refuses anything and this test would assert nothing.
        let dir = s.task_dir("TASK-0001");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();

        let mut t = task("TASK-0001");
        t.status = TaskStatus::Running;
        let err = s.put(t).expect_err("a refused write is an error");
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
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
