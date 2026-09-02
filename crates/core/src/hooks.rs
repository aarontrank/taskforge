use serde::{Deserialize, Serialize};
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// One configured hook, as stored in `config.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookConfig {
    pub id: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub event: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_filter: Option<Vec<String>>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

fn default_true() -> bool {
    true
}
fn default_timeout() -> u64 {
    10_000
}

/// Outcome of one hook invocation, destined for `hooks.log`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookResult {
    pub hook_id: String,
    pub event: String,
    pub task_id: String,
    pub ok: bool,
    pub exit_code: i32,
    pub timed_out: bool,
    /// Whether the process was launched at all. A missing binary and a hook that exited with an
    /// unknown code both report `exit_code: -1`, so without this a configuration error is
    /// indistinguishable from a hook that ran and misbehaved.
    pub started: bool,
}

/// Runs configured hooks after a mutation has already been committed.
///
/// Every failure mode — non-zero exit, missing binary, timeout — is reported rather than
/// propagated: hooks are notifications, and a broken notification must not undo the task
/// change that triggered it.
pub struct HookEngine {
    hooks: Vec<HookConfig>,
}

impl HookEngine {
    pub fn new(hooks: Vec<HookConfig>) -> Self {
        HookEngine { hooks }
    }

    /// Fire every hook matching this event and workspace, in configuration order.
    pub fn fire(
        &self,
        event: &str,
        workspace: &str,
        task_id: &str,
        status: &str,
    ) -> Vec<HookResult> {
        let payload = serde_json::json!({
            "event": event,
            "workspace": workspace,
            "task_id": task_id,
            "status": status,
        })
        .to_string();

        self.hooks
            .iter()
            .filter(|h| h.enabled && h.event == event)
            .filter(|h| match &h.workspace_filter {
                Some(list) => list.iter().any(|w| w == workspace),
                None => true,
            })
            .map(|h| Self::run(h, event, task_id, &payload))
            .collect()
    }

    fn run(h: &HookConfig, event: &str, task_id: &str, payload: &str) -> HookResult {
        let mut result = HookResult {
            hook_id: h.id.clone(),
            event: event.to_string(),
            task_id: task_id.to_string(),
            ok: false,
            exit_code: -1,
            timed_out: false,
            started: false,
        };

        let spawned = Command::new(&h.command)
            .args(&h.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();

        let mut child = match spawned {
            Ok(c) => c,
            // A missing or unexecutable command is a configuration error, reported like any
            // other hook failure — but flagged as never-started so it reads as one.
            Err(_) => return result,
        };
        result.started = true;

        if let Some(stdin) = child.stdin.take() {
            let mut stdin = stdin;
            let _ = stdin.write_all(payload.as_bytes());
            // Dropping stdin closes the pipe, so a hook reading to EOF (`cat`) terminates.
        }

        // Poll rather than block, so `timeout_ms` is actually enforced. std has no
        // wait-with-timeout, and a whole async runtime would be a large dependency for this.
        let deadline = Instant::now() + Duration::from_millis(h.timeout_ms);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    result.exit_code = status.code().unwrap_or(-1);
                    result.ok = status.success();
                    return result;
                }
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    result.timed_out = true;
                    return result;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(_) => return result,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::hooks::*;

    fn hook(id: &str, event: &str, command: &str, args: Vec<&str>) -> HookConfig {
        HookConfig {
            id: id.into(),
            enabled: true,
            event: event.into(),
            command: command.into(),
            args: args.into_iter().map(String::from).collect(),
            workspace_filter: None,
            timeout_ms: 5_000,
        }
    }

    #[test]
    fn a_matching_hook_receives_the_payload_as_json_on_stdin() {
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("captured.json");
        let h = hook(
            "capture",
            "task.status_changed",
            "sh",
            vec!["-c", &format!("cat > {}", out.display())],
        );
        let engine = HookEngine::new(vec![h]);

        let results = engine.fire("task.status_changed", "main", "TASK-0001", "running");
        assert_eq!(results.len(), 1, "the hook ran");
        assert!(results[0].ok, "exit 0: {:?}", results[0]);

        let body = std::fs::read_to_string(&out).expect("hook wrote its stdin");
        let v: serde_json::Value = serde_json::from_str(&body).expect("payload is JSON");
        assert_eq!(v["event"], "task.status_changed");
        assert_eq!(v["task_id"], "TASK-0001");
        assert_eq!(v["workspace"], "main");
        assert_eq!(v["status"], "running");
    }

    #[test]
    fn a_hook_for_another_event_does_not_fire() {
        let engine = HookEngine::new(vec![hook("x", "task.created", "true", vec![])]);
        assert!(engine
            .fire("task.status_changed", "main", "TASK-0001", "running")
            .is_empty());
    }

    #[test]
    fn a_disabled_hook_does_not_fire() {
        let mut h = hook("x", "task.created", "true", vec![]);
        h.enabled = false;
        let engine = HookEngine::new(vec![h]);
        assert!(engine
            .fire("task.created", "main", "TASK-0001", "open")
            .is_empty());
    }

    #[test]
    fn the_workspace_filter_excludes_other_workspaces() {
        let mut h = hook("x", "task.created", "true", vec![]);
        h.workspace_filter = Some(vec!["other".into()]);
        let engine = HookEngine::new(vec![h]);
        assert!(engine
            .fire("task.created", "main", "TASK-0001", "open")
            .is_empty());
        assert_eq!(
            engine
                .fire("task.created", "other", "TASK-0001", "open")
                .len(),
            1
        );
    }

    #[test]
    fn a_failing_hook_is_reported_without_stopping_the_others() {
        let engine = HookEngine::new(vec![
            hook("bad", "task.created", "false", vec![]),
            hook("good", "task.created", "true", vec![]),
        ]);
        let r = engine.fire("task.created", "main", "TASK-0001", "open");
        assert_eq!(r.len(), 2, "both attempted");
        assert!(!r[0].ok, "the failure is recorded");
        assert!(r[1].ok, "and does not prevent the next hook");
    }

    #[test]
    fn a_hook_that_overruns_its_timeout_is_killed_and_reported() {
        let mut h = hook("slow", "task.created", "sleep", vec!["30"]);
        h.timeout_ms = 200;
        let engine = HookEngine::new(vec![h]);
        let started = std::time::Instant::now();
        let r = engine.fire("task.created", "main", "TASK-0001", "open");
        assert!(!r[0].ok, "a timed-out hook is not a success");
        assert!(r[0].timed_out, "and is reported as a timeout");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "it did not wait 30s"
        );
    }

    #[test]
    fn a_command_that_does_not_exist_is_reported_not_panicked() {
        let engine = HookEngine::new(vec![hook(
            "nope",
            "task.created",
            "definitely-not-a-cmd-xyz",
            vec![],
        )]);
        let r = engine.fire("task.created", "main", "TASK-0001", "open");
        assert!(!r[0].ok);
    }

    #[test]
    fn a_command_that_does_not_exist_is_distinguishable_from_one_that_failed() {
        // Both report exit_code -1, so without this flag a missing binary is described as
        // "exited -1" — which reads as a hook that ran and returned a strange code.
        let engine = HookEngine::new(vec![
            hook("nope", "task.created", "definitely-not-a-cmd-xyz", vec![]),
            hook("ran", "task.created", "false", vec![]),
        ]);
        let r = engine.fire("task.created", "main", "TASK-0001", "open");
        assert!(!r[0].started, "the missing binary never started");
        assert!(r[1].started, "the failing hook did start");
    }

    #[test]
    fn a_timed_out_hook_counts_as_started() {
        let mut h = hook("slow", "task.created", "sleep", vec!["30"]);
        h.timeout_ms = 200;
        let engine = HookEngine::new(vec![h]);
        let r = engine.fire("task.created", "main", "TASK-0001", "open");
        assert!(r[0].started, "it ran, it just did not finish in time");
    }
}
