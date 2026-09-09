use std::process::Command;

/// Drive the built binary against a throwaway root, so no test ever touches ~/.taskforge.
fn run(root: &std::path::Path, args: &[&str]) -> (bool, serde_json::Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_taskforge"))
        .args(args)
        .env("TASKFORGE_ROOT", root)
        .output()
        .expect("binary runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let json = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout was not JSON ({e}):\n{stdout}"));
    (out.status.success(), json)
}

fn setup() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let (ok, _) = run(d.path(), &["init", "--json"]);
    assert!(ok, "init succeeds");
    let (ok, _) = run(
        d.path(),
        &[
            "owner", "add", "--name", "agent", "--type", "agent", "--json",
        ],
    );
    assert!(ok, "owner add succeeds");
    d
}

#[test]
fn every_response_uses_the_documented_envelope() {
    let d = setup();
    let (_, v) = run(d.path(), &["task", "list", "--json"]);
    for key in [
        "ok",
        "command",
        "taskforge_version",
        "data",
        "warnings",
        "errors",
    ] {
        assert!(v.get(key).is_some(), "envelope is missing {key}: {v}");
    }
    assert_eq!(v["command"], "task list");
}

#[test]
fn a_created_task_can_be_read_back_and_listed() {
    let d = setup();
    let (ok, v) = run(
        d.path(),
        &[
            "task",
            "create",
            "--title",
            "Port the CLI",
            "--owner",
            "agent",
            "--actor",
            "agent",
            "--json",
        ],
    );
    assert!(ok, "create succeeds: {v}");
    let id = v["data"]["id"]
        .as_str()
        .expect("an id is returned")
        .to_string();
    assert_eq!(id, "TASK-0001");
    assert_eq!(v["data"]["status"], "open");

    let (_, shown) = run(d.path(), &["task", "show", "--id", &id, "--json"]);
    assert_eq!(shown["data"]["title"], "Port the CLI");

    let (_, listed) = run(d.path(), &["task", "list", "--json"]);
    assert_eq!(listed["data"].as_array().unwrap().len(), 1);
}

#[test]
fn the_full_review_path_runs_start_to_accepted() {
    let d = setup();
    let (_, v) = run(
        d.path(),
        &[
            "task",
            "create",
            "--title",
            "Gated",
            "--owner",
            "agent",
            "--actor",
            "agent",
            "--review-required",
            "--json",
        ],
    );
    let id = v["data"]["id"].as_str().unwrap().to_string();

    for (args, expected) in [
        (vec!["start"], "running"),
        (vec!["request-review"], "in-review"),
        (vec!["merge"], "merged"),
        (vec!["accept"], "done"),
    ] {
        let mut cmd = vec!["task", args[0], "--id", &id, "--actor", "agent", "--json"];
        let (ok, r) = run(d.path(), &cmd.split_off(0));
        assert!(ok, "{} succeeds: {r}", args[0]);
        assert_eq!(r["data"]["status"], expected, "after {}", args[0]);
    }
}

#[test]
fn completing_a_review_required_task_directly_is_refused_with_its_code() {
    let d = setup();
    let (_, v) = run(
        d.path(),
        &[
            "task",
            "create",
            "--title",
            "Gated",
            "--owner",
            "agent",
            "--actor",
            "agent",
            "--review-required",
            "--json",
        ],
    );
    let id = v["data"]["id"].as_str().unwrap().to_string();
    run(
        d.path(),
        &["task", "start", "--id", &id, "--actor", "agent", "--json"],
    );

    let (ok, r) = run(
        d.path(),
        &[
            "task", "complete", "--id", &id, "--actor", "agent", "--json",
        ],
    );
    assert!(!ok, "the command fails");
    assert_eq!(r["ok"], false);
    assert_eq!(r["errors"][0]["code"], "REVIEW_REQUIRED", "got: {r}");
}

#[test]
fn starting_a_blocked_task_is_refused_and_names_the_blocker() {
    let d = setup();
    let mk = |title: &str| -> String {
        let (_, v) = run(
            d.path(),
            &[
                "task", "create", "--title", title, "--owner", "agent", "--actor", "agent",
                "--json",
            ],
        );
        v["data"]["id"].as_str().unwrap().to_string()
    };
    let blocker = mk("first");
    let blocked = mk("second");
    let (ok, _) = run(
        d.path(),
        &[
            "task",
            "add-blocker",
            "--id",
            &blocked,
            "--blocked-by",
            &blocker,
            "--actor",
            "agent",
            "--json",
        ],
    );
    assert!(ok, "add-blocker succeeds");

    let (ok, r) = run(
        d.path(),
        &[
            "task", "start", "--id", &blocked, "--actor", "agent", "--json",
        ],
    );
    assert!(!ok);
    assert_eq!(r["errors"][0]["code"], "BLOCKED_BY_OPEN_TASK");
    assert!(r["errors"][0]["message"]
        .as_str()
        .unwrap()
        .contains(&blocker));
}

#[test]
fn an_unknown_task_reports_task_not_found() {
    let d = setup();
    let (ok, r) = run(d.path(), &["task", "show", "--id", "TASK-9999", "--json"]);
    assert!(!ok);
    assert_eq!(r["errors"][0]["code"], "TASK_NOT_FOUND");
}

#[test]
fn review_id_and_expected_by_are_settable_and_persist() {
    let d = setup();
    let (_, v) = run(
        d.path(),
        &[
            "task",
            "create",
            "--title",
            "Board row",
            "--owner",
            "agent",
            "--actor",
            "agent",
            "--json",
        ],
    );
    let id = v["data"]["id"].as_str().unwrap().to_string();
    let (ok, _) = run(
        d.path(),
        &[
            "task",
            "set-review",
            "--id",
            &id,
            "--review-id",
            "PR-4821",
            "--expected-by",
            "2026-09-03T17:00:00Z",
            "--actor",
            "agent",
            "--json",
        ],
    );
    assert!(ok, "set-review succeeds");

    let (_, shown) = run(d.path(), &["task", "show", "--id", &id, "--json"]);
    assert_eq!(shown["data"]["reviews"][0], "PR-4821");
    assert_eq!(shown["data"]["expected_by"], "2026-09-03T17:00:00Z");
}

/// Create a task and return its id.
fn mk(root: &std::path::Path, title: &str) -> String {
    let (_, v) = run(
        root,
        &[
            "task", "create", "--title", title, "--owner", "agent", "--actor", "agent", "--json",
        ],
    );
    v["data"]["id"].as_str().unwrap().to_string()
}

#[test]
fn the_patch_style_setters_each_change_exactly_their_own_field() {
    let d = setup();
    let id = mk(d.path(), "before");
    run(
        d.path(),
        &["owner", "add", "--name", "rev", "--type", "human", "--json"],
    );

    for (args, field, expected) in [
        (vec!["set-title", "--title", "after"], "title", "after"),
        (
            vec!["set-description", "--description", "why"],
            "description",
            "why",
        ),
        (vec!["assign", "--owner", "rev"], "owner", "rev"),
        (vec!["set-reviewer", "--reviewer", "rev"], "reviewer", "rev"),
        (
            vec!["set-due", "--due-at", "2026-12-01T00:00:00Z"],
            "due_at",
            "2026-12-01T00:00:00Z",
        ),
    ] {
        let mut cmd = vec!["task", args[0], "--id", &id];
        cmd.extend_from_slice(&args[1..]);
        cmd.extend_from_slice(&["--actor", "agent", "--json"]);
        let (ok, r) = run(d.path(), &cmd);
        assert!(ok, "{} succeeds: {r}", args[0]);
        assert_eq!(r["data"][field], expected, "after {}", args[0]);
    }

    let (_, r) = run(
        d.path(),
        &[
            "task",
            "set-review-required",
            "--id",
            &id,
            "--value",
            "true",
            "--actor",
            "agent",
            "--json",
        ],
    );
    assert_eq!(r["data"]["review_required"], true);
}

#[test]
fn a_blocker_can_be_removed_again() {
    let d = setup();
    let a = mk(d.path(), "a");
    let b = mk(d.path(), "b");
    run(
        d.path(),
        &[
            "task",
            "add-blocker",
            "--id",
            &b,
            "--blocked-by",
            &a,
            "--actor",
            "agent",
            "--json",
        ],
    );
    let (ok, r) = run(
        d.path(),
        &[
            "task",
            "remove-blocker",
            "--id",
            &b,
            "--blocked-by",
            &a,
            "--actor",
            "agent",
            "--json",
        ],
    );
    assert!(ok);
    assert_eq!(r["data"]["blocked_by"].as_array().unwrap().len(), 0);
    // With the blocker gone, the task starts.
    let (ok, _) = run(
        d.path(),
        &["task", "start", "--id", &b, "--actor", "agent", "--json"],
    );
    assert!(ok, "an unblocked task starts");
}

#[test]
fn a_subtask_records_its_parent_and_appears_in_the_tree() {
    let d = setup();
    let parent = mk(d.path(), "parent");
    let (ok, v) = run(
        d.path(),
        &[
            "task", "create", "--title", "child", "--owner", "agent", "--actor", "agent",
            "--parent", &parent, "--json",
        ],
    );
    assert!(ok, "subtask create succeeds: {v}");
    assert_eq!(v["data"]["parent_task_id"], parent);

    let (_, tree) = run(d.path(), &["task", "tree", "--id", &parent, "--json"]);
    assert_eq!(tree["data"]["id"], parent);
    let kids = tree["data"]["subtasks"].as_array().expect("subtasks array");
    assert_eq!(kids.len(), 1);
    assert_eq!(kids[0]["title"], "child");
}

#[test]
fn search_matches_title_and_description_but_not_unrelated_tasks() {
    let d = setup();
    mk(d.path(), "port the storage layer");
    let other = mk(d.path(), "unrelated");
    run(
        d.path(),
        &[
            "task",
            "set-description",
            "--id",
            &other,
            "--description",
            "mentions storage",
            "--actor",
            "agent",
            "--json",
        ],
    );
    mk(d.path(), "nothing to see");

    let (_, r) = run(d.path(), &["task", "search", "--text", "storage", "--json"]);
    let hits = r["data"].as_array().unwrap();
    assert_eq!(hits.len(), 2, "title and description both match: {r}");

    let (_, none) = run(
        d.path(),
        &["task", "search", "--text", "zzznotpresent", "--json"],
    );
    assert_eq!(none["data"].as_array().unwrap().len(), 0);
}

#[test]
fn attachments_and_artifacts_record_refs_and_copy_or_link() {
    let d = setup();
    let id = mk(d.path(), "with files");
    let src = d.path().join("spec.txt");
    std::fs::write(&src, "hello").unwrap();

    let (ok, r) = run(
        d.path(),
        &[
            "task",
            "add-attachment",
            "--id",
            &id,
            "--path",
            src.to_str().unwrap(),
            "--mode",
            "copy",
            "--actor",
            "agent",
            "--json",
        ],
    );
    assert!(ok, "copy attachment: {r}");
    assert_eq!(r["data"]["attachment_refs"].as_array().unwrap().len(), 1);
    let copied = d
        .path()
        .join("workspaces/main/tasks")
        .join(&id)
        .join("attachments/spec.txt");
    assert!(
        copied.is_file(),
        "copy mode places the file in the task folder"
    );

    let (ok, r) = run(
        d.path(),
        &[
            "task",
            "add-artifact",
            "--id",
            &id,
            "--path",
            src.to_str().unwrap(),
            "--mode",
            "link",
            "--actor",
            "agent",
            "--json",
        ],
    );
    assert!(ok, "link artifact: {r}");
    assert_eq!(
        r["data"]["artifact_refs"][0],
        src.to_str().unwrap(),
        "link mode stores the path"
    );
    let linked = d
        .path()
        .join("workspaces/main/tasks")
        .join(&id)
        .join("artifacts/spec.txt");
    assert!(!linked.exists(), "link mode copies nothing");
}

#[test]
fn a_missing_attachment_source_is_reported_not_silently_recorded() {
    let d = setup();
    let id = mk(d.path(), "x");
    let (ok, r) = run(
        d.path(),
        &[
            "task",
            "add-attachment",
            "--id",
            &id,
            "--path",
            "/nope/missing.txt",
            "--mode",
            "copy",
            "--actor",
            "agent",
            "--json",
        ],
    );
    assert!(!ok);
    assert_eq!(r["errors"][0]["code"], "ATTACHMENT_NOT_FOUND");
}

#[test]
fn archive_and_soft_delete_set_flags_and_hide_from_the_default_listing() {
    let d = setup();
    let keep = mk(d.path(), "keep");
    let gone = mk(d.path(), "archive me");
    let deleted = mk(d.path(), "delete me");

    let (ok, r) = run(
        d.path(),
        &[
            "task", "archive", "--id", &gone, "--actor", "agent", "--json",
        ],
    );
    assert!(ok);
    assert_eq!(r["data"]["archived"], true);
    // The new model keeps status orthogonal: archiving does not overwrite it.
    assert_eq!(
        r["data"]["status"], "open",
        "archived is a flag, not a status"
    );

    run(
        d.path(),
        &[
            "task",
            "soft-delete",
            "--id",
            &deleted,
            "--actor",
            "agent",
            "--json",
        ],
    );

    let (_, listed) = run(d.path(), &["task", "list", "--json"]);
    let ids: Vec<&str> = listed["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        vec![keep.as_str()],
        "only the live task is listed by default"
    );

    let (_, all) = run(d.path(), &["task", "list", "--archived", "--json"]);
    assert!(all["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["id"] == gone.as_str()));
}

#[test]
fn workspaces_can_be_created_and_listed_and_isolate_their_tasks() {
    let d = setup();
    mk(d.path(), "in main");
    let (ok, _) = run(d.path(), &["workspace", "add", "--name", "side", "--json"]);
    assert!(ok, "workspace add succeeds");

    let (_, ws) = run(d.path(), &["workspace", "list", "--json"]);
    let names: Vec<&str> = ws["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"main") && names.contains(&"side"),
        "got {names:?}"
    );

    let (_, side) = run(d.path(), &["--workspace", "side", "task", "list", "--json"]);
    assert_eq!(
        side["data"].as_array().unwrap().len(),
        0,
        "the new workspace is empty"
    );
}

#[test]
fn the_audit_trail_is_readable_through_the_cli() {
    let d = setup();
    let id = mk(d.path(), "audited");
    run(
        d.path(),
        &["task", "start", "--id", &id, "--actor", "aaron", "--json"],
    );
    let (ok, r) = run(d.path(), &["task", "audit", "--id", &id, "--json"]);
    assert!(ok);
    let entries = r["data"].as_array().unwrap();
    assert!(!entries.is_empty(), "the status change is recorded");
    assert_eq!(entries.last().unwrap()["to"], "running");
    assert_eq!(entries.last().unwrap()["actor"], "aaron");
}

#[test]
fn a_hook_configured_in_config_json_fires_on_a_real_status_change() {
    let d = setup();
    let marker = d.path().join("hook-ran.json");
    let config = serde_json::json!({
        "default_workspace": "main",
        "hooks": [{
            "id": "capture",
            "enabled": true,
            "event": "task.status_changed",
            "command": "sh",
            "args": ["-c", format!("cat > {}", marker.display())],
            "timeout_ms": 5000
        }]
    });
    std::fs::write(d.path().join("config.json"), config.to_string()).unwrap();

    let id = mk(d.path(), "hooked");
    let (ok, _) = run(
        d.path(),
        &["task", "start", "--id", &id, "--actor", "agent", "--json"],
    );
    assert!(ok);

    let body = std::fs::read_to_string(&marker).expect("the configured hook actually ran");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["task_id"], id);
    assert_eq!(v["status"], "running");
}

#[test]
fn archiving_an_already_archived_task_is_refused() {
    let d = setup();
    let id = mk(d.path(), "x");
    run(
        d.path(),
        &["task", "archive", "--id", &id, "--actor", "agent", "--json"],
    );
    let (ok, r) = run(
        d.path(),
        &["task", "archive", "--id", &id, "--actor", "agent", "--json"],
    );
    assert!(!ok);
    assert_eq!(r["errors"][0]["code"], "ALREADY_ARCHIVED");
}

#[test]
fn an_unrecognized_status_filter_is_reported() {
    let d = setup();
    let (ok, r) = run(
        d.path(),
        &["task", "list", "--status", "in_progress", "--json"],
    );
    assert!(!ok, "the old snake_case name is no longer a status");
    assert_eq!(r["errors"][0]["code"], "INVALID_STATUS");
}

#[test]
fn creating_a_task_for_an_unregistered_owner_is_refused() {
    let d = setup();
    let (ok, r) = run(
        d.path(),
        &[
            "task", "create", "--title", "x", "--owner", "ghost", "--actor", "agent", "--json",
        ],
    );
    assert!(!ok);
    assert_eq!(r["errors"][0]["code"], "OWNER_NOT_FOUND");
}

#[test]
fn registering_the_same_owner_twice_is_refused() {
    let d = setup();
    let (ok, r) = run(
        d.path(),
        &[
            "owner", "add", "--name", "agent", "--type", "agent", "--json",
        ],
    );
    assert!(!ok);
    assert_eq!(r["errors"][0]["code"], "OWNER_EXISTS");
}

#[test]
fn assigning_to_an_unregistered_owner_is_refused() {
    let d = setup();
    let id = mk(d.path(), "x");
    let (ok, r) = run(
        d.path(),
        &[
            "task", "assign", "--id", &id, "--owner", "ghost", "--actor", "agent", "--json",
        ],
    );
    assert!(!ok);
    assert_eq!(r["errors"][0]["code"], "OWNER_NOT_FOUND");
}

#[test]
fn creating_a_subtask_of_a_nonexistent_parent_is_refused() {
    let d = setup();
    let (ok, r) = run(
        d.path(),
        &[
            "task",
            "create",
            "--title",
            "orphan",
            "--owner",
            "agent",
            "--actor",
            "agent",
            "--parent",
            "TASK-9999",
            "--json",
        ],
    );
    assert!(!ok);
    assert_eq!(r["errors"][0]["code"], "TASK_NOT_FOUND");
}

/// Every status in the model must be reachable through the CLI.
///
/// The model having eleven states buys nothing if the binary can only produce six of them.
/// This walks a real path to each one, so a state that exists only in the enum is a failure.
#[test]
fn every_status_in_the_model_is_reachable_through_the_cli() {
    let d = setup();
    let paths: &[(&str, &[&str])] = &[
        ("open", &[]),
        ("pending", &["pending"]),
        ("running", &["start"]),
        ("in-review", &["start", "request-review"]),
        ("changes-requested", &["start", "request-review", "reject"]),
        ("merged", &["start", "request-review", "merge"]),
        ("done", &["start", "request-review", "merge", "accept"]),
        ("waiting-on-schedule", &["start", "wait"]),
        ("stuck", &["start", "block"]),
        ("failed", &["start", "fail"]),
        ("cancelled", &["cancel"]),
    ];

    for (want, steps) in paths {
        let id = mk(d.path(), want);
        for step in *steps {
            let (ok, r) = run(
                d.path(),
                &["task", step, "--id", &id, "--actor", "agent", "--json"],
            );
            assert!(ok, "`task {step}` on the way to {want} failed: {r}");
        }
        let (_, shown) = run(d.path(), &["task", "show", "--id", &id, "--json"]);
        assert_eq!(
            shown["data"]["status"], *want,
            "path {steps:?} should end at {want}"
        );
    }
}

#[test]
fn set_status_refuses_a_move_the_transition_table_forbids() {
    let d = setup();
    let id = mk(d.path(), "x");
    // open -> merged is not legal, and the generic setter must be guarded exactly like the
    // named commands rather than being a way around them.
    let (ok, r) = run(
        d.path(),
        &[
            "task",
            "set-status",
            "--id",
            &id,
            "--status",
            "merged",
            "--actor",
            "agent",
            "--json",
        ],
    );
    assert!(!ok);
    assert_eq!(r["errors"][0]["code"], "INVALID_STATUS_TRANSITION");
}

#[test]
fn set_status_rejects_a_status_name_that_does_not_exist() {
    let d = setup();
    let id = mk(d.path(), "x");
    let (ok, _) = run(
        d.path(),
        &[
            "task",
            "set-status",
            "--id",
            &id,
            "--status",
            "bogus",
            "--actor",
            "agent",
            "--json",
        ],
    );
    assert!(!ok, "an unknown status name must not be accepted");
}

#[test]
fn the_dev_checkout_is_a_separate_field_from_the_task_partition() {
    let d = setup();
    let id = mk(d.path(), "S1");
    // `workspace` partitions the task store (main, side). `checkout` is the dev workspace the
    // work happens in (a numbered dev workspace, a worktree name). Conflating them would shard
    // the task store one-per-stream and break `task list`.
    let (ok, r) = run(
        d.path(),
        &[
            "task",
            "set-worker",
            "--id",
            &id,
            "--worker",
            "sandbox-3",
            "--checkout",
            "3",
            "--actor",
            "agent",
            "--json",
        ],
    );
    assert!(ok, "set-worker succeeds: {r}");
    assert_eq!(r["data"]["worker"], "sandbox-3");
    assert_eq!(r["data"]["checkout"], "3");
    assert_eq!(
        r["data"]["workspace"], "main",
        "the task partition is untouched"
    );

    let (_, shown) = run(d.path(), &["task", "show", "--id", &id, "--json"]);
    assert_eq!(shown["data"]["checkout"], "3", "and it persists");
}

#[test]
fn set_worker_and_set_review_each_leave_the_other_pair_alone() {
    let d = setup();
    let id = mk(d.path(), "S1");
    run(
        d.path(),
        &[
            "task",
            "set-review",
            "--id",
            &id,
            "--review-id",
            "PR-1",
            "--expected-by",
            "2026-09-03T17:00:00Z",
            "--actor",
            "agent",
            "--json",
        ],
    );
    run(
        d.path(),
        &[
            "task",
            "set-worker",
            "--id",
            &id,
            "--worker",
            "w1",
            "--checkout",
            "7",
            "--actor",
            "agent",
            "--json",
        ],
    );

    let (_, r) = run(d.path(), &["task", "show", "--id", &id, "--json"]);
    assert_eq!(
        r["data"]["reviews"][0], "PR-1",
        "set-worker did not clear the review fields"
    );
    assert_eq!(r["data"]["expected_by"], "2026-09-03T17:00:00Z");
    assert_eq!(r["data"]["worker"], "w1");
    assert_eq!(r["data"]["checkout"], "7");

    // And an omitted flag leaves its own field alone rather than nulling it.
    run(
        d.path(),
        &[
            "task",
            "set-worker",
            "--id",
            &id,
            "--checkout",
            "8",
            "--actor",
            "agent",
            "--json",
        ],
    );
    let (_, r2) = run(d.path(), &["task", "show", "--id", &id, "--json"]);
    assert_eq!(
        r2["data"]["worker"], "w1",
        "worker survived a checkout-only update"
    );
    assert_eq!(r2["data"]["checkout"], "8");
}

/// Run without `--json` and capture stdout/stderr/exit separately.
fn run_text(root: &std::path::Path, args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_taskforge"))
        .args(args)
        .env("TASKFORGE_ROOT", root)
        .output()
        .expect("binary runs");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

#[test]
fn without_json_the_list_is_a_readable_table_not_an_envelope() {
    let d = setup();
    let id = mk(d.path(), "port the storage layer");
    run(
        d.path(),
        &["task", "start", "--id", &id, "--actor", "agent", "--json"],
    );

    let (ok, out, _) = run_text(d.path(), &["task", "list"]);
    assert!(ok);
    assert!(
        !out.contains("\"taskforge_version\""),
        "no JSON envelope:\n{out}"
    );
    assert!(!out.trim_start().starts_with('{'), "not JSON:\n{out}");
    assert!(
        out.contains("ID") && out.contains("STATUS"),
        "has a header:\n{out}"
    );
    assert!(
        out.contains(&id) && out.contains("running"),
        "has the row:\n{out}"
    );
    assert!(
        out.contains("port the storage layer"),
        "has the title:\n{out}"
    );
    // A board needs the execution columns, not just id and title.
    for col in ["WORKER", "REVIEW", "EXPECTED-BY"] {
        assert!(out.contains(col), "board column {col} missing:\n{out}");
    }
}

#[test]
fn without_json_show_prints_the_fields_a_human_wants() {
    let d = setup();
    let id = mk(d.path(), "a stream");
    run(
        d.path(),
        &[
            "task",
            "set-worker",
            "--id",
            &id,
            "--worker",
            "w1",
            "--checkout",
            "3",
            "--actor",
            "agent",
            "--json",
        ],
    );
    let (ok, out, _) = run_text(d.path(), &["task", "show", "--id", &id]);
    assert!(ok);
    assert!(!out.trim_start().starts_with('{'), "not JSON:\n{out}");
    for needle in [id.as_str(), "a stream", "open", "w1", "3"] {
        assert!(out.contains(needle), "{needle:?} missing from:\n{out}");
    }
}

#[test]
fn json_still_produces_the_envelope_when_asked() {
    let d = setup();
    mk(d.path(), "x");
    let (_, out, _) = run_text(d.path(), &["task", "list", "--json"]);
    assert!(
        out.trim_start().starts_with('{'),
        "still JSON on demand:\n{out}"
    );
    assert!(out.contains("\"taskforge_version\""));
}

#[test]
fn without_json_a_mutation_confirms_in_one_line() {
    let d = setup();
    let (ok, out, _) = run_text(
        d.path(),
        &[
            "task",
            "create",
            "--title",
            "new thing",
            "--owner",
            "agent",
            "--actor",
            "agent",
        ],
    );
    assert!(ok);
    assert!(out.lines().count() <= 2, "one line, not a dump:\n{out}");
    assert!(out.contains("TASK-0001"), "names the new task:\n{out}");
}

#[test]
fn without_json_an_error_goes_to_stderr_with_its_code_and_exits_nonzero() {
    let d = setup();
    let (ok, out, err) = run_text(d.path(), &["task", "show", "--id", "TASK-9999"]);
    assert!(!ok, "exit is non-zero");
    assert!(err.contains("TASK_NOT_FOUND"), "code on stderr:\n{err}");
    assert!(out.is_empty(), "nothing on stdout for a failure:\n{out}");
}

// ---------------------------------------------------------------------------
// D1 — a write that fails must not be reported as success.
// ---------------------------------------------------------------------------

/// Make a task's `task.md` unwritable, run `f`, then restore permissions.
///
/// Unix-only: it works by dropping the write bit, which is the closest portable stand-in for
/// the full disk / read-only mount that produces this failure in the wild.
#[cfg(unix)]
fn with_unwritable_task<T>(root: &std::path::Path, id: &str, f: impl FnOnce() -> T) -> T {
    use std::os::unix::fs::PermissionsExt;
    let file = root.join("workspaces/main/tasks").join(id).join("task.md");
    let original = std::fs::metadata(&file).unwrap().permissions();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o444)).unwrap();
    let out = f();
    std::fs::set_permissions(&file, original).unwrap();
    out
}

#[cfg(unix)]
#[test]
fn a_transition_whose_write_fails_reports_io_error_and_exits_nonzero() {
    let d = setup();
    let id = mk(d.path(), "victim");
    let (ok, v) = with_unwritable_task(d.path(), &id, || {
        run(
            d.path(),
            &["task", "start", "--id", &id, "--actor", "agent", "--json"],
        )
    });
    assert!(!ok, "a lost write is a failure, not a success: {v}");
    assert_eq!(v["errors"][0]["code"], "IO_ERROR", "{v}");
}

#[cfg(unix)]
#[test]
fn a_transition_whose_write_fails_does_not_report_a_status_it_did_not_store() {
    let d = setup();
    let id = mk(d.path(), "victim");
    with_unwritable_task(d.path(), &id, || {
        run(
            d.path(),
            &["task", "start", "--id", &id, "--actor", "agent", "--json"],
        )
    });
    // The bug was that the CLI announced running/v2 while the file still said open/v1.
    let (_, shown) = run(d.path(), &["task", "show", "--id", &id, "--json"]);
    assert_eq!(shown["data"]["status"], "open");
    assert_eq!(shown["data"]["version"], 1);
}

#[cfg(unix)]
#[test]
fn a_patch_whose_write_fails_reports_io_error() {
    let d = setup();
    let id = mk(d.path(), "victim");
    let (ok, v) = with_unwritable_task(d.path(), &id, || {
        run(
            d.path(),
            &[
                "task",
                "set-title",
                "--id",
                &id,
                "--title",
                "new",
                "--actor",
                "agent",
                "--json",
            ],
        )
    });
    assert!(!ok, "{v}");
    assert_eq!(v["errors"][0]["code"], "IO_ERROR", "{v}");
}

// ---------------------------------------------------------------------------
// D2 — --actor is required on patch commands, so it must reach the audit log.
// ---------------------------------------------------------------------------

/// Actions recorded in a task's audit log, in order.
fn audit_actions(root: &std::path::Path, id: &str) -> Vec<String> {
    let (_, v) = run(root, &["task", "audit", "--id", id, "--json"]);
    v["data"]
        .as_array()
        .expect("audit is a list")
        .iter()
        .map(|e| e["action"].as_str().unwrap_or_default().to_string())
        .collect()
}

#[test]
fn creating_a_task_is_audited_against_its_actor() {
    let d = setup();
    let id = mk(d.path(), "audited");
    let (_, v) = run(d.path(), &["task", "audit", "--id", &id, "--json"]);
    let entries = v["data"].as_array().expect("audit is a list");
    assert_eq!(entries.len(), 1, "creation is history too: {v}");
    assert_eq!(entries[0]["action"], "created");
    assert_eq!(entries[0]["actor"], "agent");
}

#[test]
fn a_field_patch_records_its_action_and_actor_in_the_audit_log() {
    let d = setup();
    let id = mk(d.path(), "audited");
    run(
        d.path(),
        &[
            "task",
            "set-review",
            "--id",
            &id,
            "--review-id",
            "PR-1",
            "--actor",
            "aaron",
            "--json",
        ],
    );
    let (_, v) = run(d.path(), &["task", "audit", "--id", &id, "--json"]);
    let entries = v["data"].as_array().unwrap();
    let patch = entries
        .iter()
        .find(|e| e["action"] == "set_review")
        .unwrap_or_else(|| panic!("set_review is in the log: {v}"));
    assert_eq!(patch["actor"], "aaron", "the actor is the one passed");
}

#[test]
fn every_patch_command_leaves_its_own_action_in_the_audit_log() {
    // One task walked through every patch-style setter: the audit log must name each one, so
    // "who changed this field" is answerable for all of them rather than a subset.
    let d = setup();
    let id = mk(d.path(), "audited");
    let blocker = mk(d.path(), "blocker");
    run(
        d.path(),
        &["owner", "add", "--name", "rev", "--type", "human", "--json"],
    );
    let steps: Vec<(&str, Vec<&str>)> = vec![
        ("set_title", vec!["set-title", "--title", "t2"]),
        (
            "set_description",
            vec!["set-description", "--description", "d"],
        ),
        ("assign", vec!["assign", "--owner", "rev"]),
        ("set_reviewer", vec!["set-reviewer", "--reviewer", "rev"]),
        (
            "set_due",
            vec!["set-due", "--due-at", "2026-12-01T00:00:00Z"],
        ),
        (
            "set_review_required",
            vec!["set-review-required", "--value", "true"],
        ),
        ("set_review", vec!["set-review", "--review-id", "CR-9"]),
        ("set_worker", vec!["set-worker", "--worker", "w1"]),
        ("add_blocker", vec!["add-blocker", "--blocked-by", &blocker]),
        (
            "remove_blocker",
            vec!["remove-blocker", "--blocked-by", &blocker],
        ),
        (
            "set_recurrence",
            vec!["set-recurrence", "--frequency", "weekly"],
        ),
        ("clear_recurrence", vec!["clear-recurrence"]),
        ("archive", vec!["archive"]),
        ("soft_delete", vec!["soft-delete"]),
    ];
    for (_, args) in &steps {
        let mut argv = vec!["task"];
        argv.extend(args.iter().copied());
        argv.extend(["--id", &id, "--actor", "aaron", "--json"]);
        let (ok, v) = run(d.path(), &argv);
        assert!(ok, "{argv:?} succeeds: {v}");
    }
    let actions = audit_actions(d.path(), &id);
    for (expected, _) in &steps {
        assert!(
            actions.iter().any(|a| a == expected),
            "{expected:?} missing from audit log {actions:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// D3 — patch commands need the same optimistic-concurrency guard as transitions.
// ---------------------------------------------------------------------------

#[test]
fn a_patch_with_a_stale_version_is_a_conflict() {
    let d = setup();
    let id = mk(d.path(), "contended");
    let (ok, v) = run(
        d.path(),
        &[
            "task",
            "set-title",
            "--id",
            &id,
            "--title",
            "x",
            "--actor",
            "aaron",
            "--version",
            "99",
            "--json",
        ],
    );
    assert!(!ok, "{v}");
    assert_eq!(v["errors"][0]["code"], "CONFLICT_VERSION_MISMATCH", "{v}");
}

#[test]
fn a_patch_with_a_stale_version_does_not_write() {
    let d = setup();
    let id = mk(d.path(), "contended");
    run(
        d.path(),
        &[
            "task",
            "set-title",
            "--id",
            &id,
            "--title",
            "clobbered",
            "--actor",
            "aaron",
            "--version",
            "99",
            "--json",
        ],
    );
    let (_, shown) = run(d.path(), &["task", "show", "--id", &id, "--json"]);
    assert_eq!(
        shown["data"]["title"], "contended",
        "the title is untouched"
    );
    assert_eq!(shown["data"]["version"], 1);
}

#[test]
fn a_patch_with_the_matching_version_is_accepted() {
    let d = setup();
    let id = mk(d.path(), "contended");
    let (ok, v) = run(
        d.path(),
        &[
            "task",
            "set-title",
            "--id",
            &id,
            "--title",
            "fresh",
            "--actor",
            "aaron",
            "--version",
            "1",
            "--json",
        ],
    );
    assert!(ok, "{v}");
    assert_eq!(v["data"]["title"], "fresh");
    assert_eq!(v["data"]["version"], 2);
}

#[test]
fn a_patch_without_a_version_still_works_unguarded() {
    // The guard is opt-in: omitting --version must not start failing.
    let d = setup();
    let id = mk(d.path(), "contended");
    let (ok, v) = run(
        d.path(),
        &[
            "task",
            "set-title",
            "--id",
            &id,
            "--title",
            "fresh",
            "--actor",
            "aaron",
            "--json",
        ],
    );
    assert!(ok, "{v}");
    assert_eq!(v["data"]["title"], "fresh");
}

// ---------------------------------------------------------------------------
// D4 — a hook that fails must be reported, not silently swallowed.
// ---------------------------------------------------------------------------

/// Install `hooks` into the root's config.json.
fn write_hooks(root: &std::path::Path, hooks: serde_json::Value) {
    let cfg = serde_json::json!({"default_workspace": "main", "hooks": hooks});
    std::fs::write(
        root.join("config.json"),
        serde_json::to_string_pretty(&cfg).unwrap(),
    )
    .unwrap();
}

#[test]
fn a_hook_that_exits_nonzero_is_reported_as_a_warning() {
    let d = setup();
    let id = mk(d.path(), "hooked");
    write_hooks(
        d.path(),
        serde_json::json!([{
            "id": "bad", "enabled": true, "event": "task.status_changed",
            "command": "sh", "args": ["-c", "exit 7"], "timeout_ms": 5000
        }]),
    );
    let (ok, v) = run(
        d.path(),
        &["task", "start", "--id", &id, "--actor", "agent", "--json"],
    );
    assert!(ok, "the mutation still stands: {v}");
    assert_eq!(v["data"]["status"], "running");
    let warnings = v["warnings"].as_array().expect("warnings is a list");
    assert_eq!(warnings.len(), 1, "the failure is surfaced: {v}");
    assert_eq!(warnings[0]["code"], "HOOK_FAILED");
    assert_eq!(warnings[0]["detail"]["hook_id"], "bad");
    assert_eq!(warnings[0]["detail"]["exit_code"], 7);
    assert_eq!(warnings[0]["detail"]["timed_out"], false);
    assert_eq!(warnings[0]["detail"]["started"], true);
}

#[test]
fn a_hook_that_times_out_is_reported_as_a_timeout() {
    let d = setup();
    let id = mk(d.path(), "hooked");
    write_hooks(
        d.path(),
        serde_json::json!([{
            "id": "slow", "enabled": true, "event": "task.status_changed",
            "command": "sh", "args": ["-c", "sleep 5"], "timeout_ms": 200
        }]),
    );
    let (ok, v) = run(
        d.path(),
        &["task", "start", "--id", &id, "--actor", "agent", "--json"],
    );
    assert!(ok, "{v}");
    assert_eq!(v["warnings"][0]["code"], "HOOK_FAILED", "{v}");
    assert_eq!(v["warnings"][0]["detail"]["timed_out"], true, "{v}");
}

#[test]
fn a_hook_naming_a_missing_binary_is_reported() {
    let d = setup();
    let id = mk(d.path(), "hooked");
    write_hooks(
        d.path(),
        serde_json::json!([{
            "id": "gone", "enabled": true, "event": "task.status_changed",
            "command": "definitely-not-a-real-binary-xyz", "args": [], "timeout_ms": 1000
        }]),
    );
    let (ok, v) = run(
        d.path(),
        &["task", "start", "--id", &id, "--actor", "agent", "--json"],
    );
    assert!(ok, "{v}");
    assert_eq!(v["warnings"][0]["code"], "HOOK_FAILED", "{v}");
    assert_eq!(v["warnings"][0]["detail"]["hook_id"], "gone");
    assert_eq!(
        v["warnings"][0]["detail"]["started"], false,
        "a missing binary reads as never-started, not as a strange exit code: {v}"
    );
    assert!(
        v["warnings"][0]["message"]
            .as_str()
            .unwrap()
            .contains("could not be started"),
        "{v}"
    );
}

#[test]
fn a_hook_that_succeeds_produces_no_warning() {
    let d = setup();
    let id = mk(d.path(), "hooked");
    write_hooks(
        d.path(),
        serde_json::json!([{
            "id": "good", "enabled": true, "event": "task.status_changed",
            "command": "true", "args": [], "timeout_ms": 5000
        }]),
    );
    let (ok, v) = run(
        d.path(),
        &["task", "start", "--id", &id, "--actor", "agent", "--json"],
    );
    assert!(ok, "{v}");
    assert!(
        v["warnings"].as_array().unwrap().is_empty(),
        "a working hook is not news: {v}"
    );
}

// ---------------------------------------------------------------------------
// D5 — the one-level nesting rule has to be enforced, not just documented.
// ---------------------------------------------------------------------------

#[test]
fn a_subtask_may_not_itself_be_given_a_subtask() {
    let d = setup();
    let parent = mk(d.path(), "parent");
    let (ok, child) = run(
        d.path(),
        &[
            "task", "create", "--title", "child", "--owner", "agent", "--actor", "agent",
            "--parent", &parent, "--json",
        ],
    );
    assert!(ok, "one level is fine: {child}");
    let child_id = child["data"]["id"].as_str().unwrap().to_string();

    let (ok, v) = run(
        d.path(),
        &[
            "task",
            "create",
            "--title",
            "grandchild",
            "--owner",
            "agent",
            "--actor",
            "agent",
            "--parent",
            &child_id,
            "--json",
        ],
    );
    assert!(!ok, "a second level must be refused: {v}");
    assert_eq!(v["errors"][0]["code"], "INVALID_PARENT", "{v}");
}

#[test]
fn refusing_a_grandchild_creates_no_task() {
    let d = setup();
    let parent = mk(d.path(), "parent");
    let (_, child) = run(
        d.path(),
        &[
            "task", "create", "--title", "child", "--owner", "agent", "--actor", "agent",
            "--parent", &parent, "--json",
        ],
    );
    let child_id = child["data"]["id"].as_str().unwrap().to_string();
    run(
        d.path(),
        &[
            "task",
            "create",
            "--title",
            "grandchild",
            "--owner",
            "agent",
            "--actor",
            "agent",
            "--parent",
            &child_id,
            "--json",
        ],
    );
    let (_, listed) = run(d.path(), &["task", "list", "--json"]);
    assert_eq!(
        listed["data"].as_array().unwrap().len(),
        2,
        "only parent and child exist: {listed}"
    );
}

/// Make a task's `audit.log` unwritable while leaving `task.md` writable, so the mutation
/// commits and only the audit append fails.
#[cfg(unix)]
fn with_unwritable_audit<T>(root: &std::path::Path, id: &str, f: impl FnOnce() -> T) -> T {
    use std::os::unix::fs::PermissionsExt;
    let log = root
        .join("workspaces/main/tasks")
        .join(id)
        .join("audit.log");
    let original = std::fs::metadata(&log).unwrap().permissions();
    std::fs::set_permissions(&log, std::fs::Permissions::from_mode(0o444)).unwrap();
    let out = f();
    std::fs::set_permissions(&log, original).unwrap();
    out
}

#[cfg(unix)]
#[test]
fn a_patch_whose_audit_append_fails_still_commits_and_warns() {
    // Asymmetric with the task write on purpose: this failure happens after the change is
    // already stored, so reporting an error would deny a mutation that really occurred.
    let d = setup();
    let id = mk(d.path(), "trail");
    let (ok, v) = with_unwritable_audit(d.path(), &id, || {
        run(
            d.path(),
            &[
                "task",
                "set-title",
                "--id",
                &id,
                "--title",
                "kept",
                "--actor",
                "aaron",
                "--json",
            ],
        )
    });
    assert!(ok, "the mutation stands: {v}");
    assert_eq!(v["data"]["title"], "kept");
    assert_eq!(v["warnings"][0]["code"], "AUDIT_WRITE_FAILED", "{v}");

    let (_, shown) = run(d.path(), &["task", "show", "--id", &id, "--json"]);
    assert_eq!(shown["data"]["title"], "kept", "and is on disk");
}

#[cfg(unix)]
#[test]
fn a_transition_whose_audit_append_fails_still_commits_and_warns() {
    let d = setup();
    let id = mk(d.path(), "trail");
    let (ok, v) = with_unwritable_audit(d.path(), &id, || {
        run(
            d.path(),
            &["task", "start", "--id", &id, "--actor", "agent", "--json"],
        )
    });
    assert!(ok, "the mutation stands: {v}");
    assert_eq!(v["data"]["status"], "running");
    assert_eq!(v["warnings"][0]["code"], "AUDIT_WRITE_FAILED", "{v}");
}

// ---------------------------------------------------------------------------
// S1 — capture schema: kind, tags, ticket, plural reviews.
// ---------------------------------------------------------------------------

#[test]
fn a_task_can_be_classified_at_creation() {
    let d = setup();
    let (ok, v) = run(
        d.path(),
        &[
            "task",
            "create",
            "--title",
            "fix the parser",
            "--owner",
            "agent",
            "--actor",
            "agent",
            "--kind",
            "bug",
            "--ticket",
            "T-99",
            "--tag",
            "parser",
            "--tag",
            "backend",
            "--json",
        ],
    );
    assert!(ok, "{v}");
    assert_eq!(v["data"]["kind"], "bug");
    assert_eq!(v["data"]["ticket"], "T-99");
    assert_eq!(v["data"]["tags"][0], "parser");
    assert_eq!(v["data"]["tags"][1], "backend");
}

#[test]
fn a_task_created_without_a_kind_is_unclassified_rather_than_guessed() {
    let d = setup();
    let id = mk(d.path(), "unclassified");
    let (_, v) = run(d.path(), &["task", "show", "--id", &id, "--json"]);
    assert!(v["data"]["kind"].is_null(), "{v}");
    assert_eq!(v["data"]["tags"].as_array().unwrap().len(), 0);
}

#[test]
fn an_unknown_kind_is_refused_at_creation() {
    let d = setup();
    let (ok, v) = run(
        d.path(),
        &[
            "task", "create", "--title", "x", "--owner", "agent", "--actor", "agent", "--kind",
            "bugfix", "--json",
        ],
    );
    assert!(!ok, "a kind off the closed set must be refused: {v}");
    assert_eq!(v["errors"][0]["code"], "INVALID_KIND", "{v}");
}

#[test]
fn refusing_an_unknown_kind_creates_no_task() {
    let d = setup();
    run(
        d.path(),
        &[
            "task", "create", "--title", "x", "--owner", "agent", "--actor", "agent", "--kind",
            "nope", "--json",
        ],
    );
    let (_, listed) = run(d.path(), &["task", "list", "--json"]);
    assert_eq!(listed["data"].as_array().unwrap().len(), 0, "{listed}");
}

#[test]
fn kind_ticket_and_tags_are_settable_after_creation() {
    let d = setup();
    let id = mk(d.path(), "classify me");
    for args in [
        vec!["set-kind", "--kind", "investigation"],
        vec!["set-ticket", "--ticket", "T-7"],
        vec!["add-tag", "--tag", "alpha"],
        vec!["add-tag", "--tag", "beta"],
    ] {
        let mut argv = vec!["task"];
        argv.extend(args);
        argv.extend(["--id", &id, "--actor", "aaron", "--json"]);
        let (ok, v) = run(d.path(), &argv);
        assert!(ok, "{argv:?}: {v}");
    }
    let (_, v) = run(d.path(), &["task", "show", "--id", &id, "--json"]);
    assert_eq!(v["data"]["kind"], "investigation");
    assert_eq!(v["data"]["ticket"], "T-7");
    assert_eq!(v["data"]["tags"].as_array().unwrap().len(), 2);

    let (ok, v) = run(
        d.path(),
        &[
            "task",
            "remove-tag",
            "--id",
            &id,
            "--tag",
            "alpha",
            "--actor",
            "aaron",
            "--json",
        ],
    );
    assert!(ok, "{v}");
    assert_eq!(v["data"]["tags"].as_array().unwrap(), &["beta"]);
}

#[test]
fn adding_the_same_tag_twice_does_not_duplicate_it() {
    let d = setup();
    let id = mk(d.path(), "tagged");
    for _ in 0..2 {
        run(
            d.path(),
            &[
                "task", "add-tag", "--id", &id, "--tag", "same", "--actor", "aaron", "--json",
            ],
        );
    }
    let (_, v) = run(d.path(), &["task", "show", "--id", &id, "--json"]);
    assert_eq!(v["data"]["tags"].as_array().unwrap(), &["same"], "{v}");
}

#[test]
fn a_task_can_carry_several_reviews() {
    // One task spanning several packages needs one review each; a single field cannot say that.
    let d = setup();
    let id = mk(d.path(), "spans packages");
    for r in ["PR-1", "PR-2"] {
        let (ok, v) = run(
            d.path(),
            &[
                "task",
                "add-review",
                "--id",
                &id,
                "--review-id",
                r,
                "--actor",
                "agent",
                "--json",
            ],
        );
        assert!(ok, "{v}");
    }
    let (_, v) = run(d.path(), &["task", "show", "--id", &id, "--json"]);
    assert_eq!(
        v["data"]["reviews"].as_array().unwrap(),
        &["PR-1", "PR-2"],
        "{v}"
    );

    let (ok, v) = run(
        d.path(),
        &[
            "task",
            "remove-review",
            "--id",
            &id,
            "--review-id",
            "PR-1",
            "--actor",
            "agent",
            "--json",
        ],
    );
    assert!(ok, "{v}");
    assert_eq!(v["data"]["reviews"].as_array().unwrap(), &["PR-2"]);
}

#[test]
fn set_review_replaces_the_whole_list() {
    let d = setup();
    let id = mk(d.path(), "one review");
    for r in ["PR-1", "PR-2"] {
        run(
            d.path(),
            &[
                "task",
                "add-review",
                "--id",
                &id,
                "--review-id",
                r,
                "--actor",
                "agent",
                "--json",
            ],
        );
    }
    let (ok, v) = run(
        d.path(),
        &[
            "task",
            "set-review",
            "--id",
            &id,
            "--review-id",
            "PR-9",
            "--actor",
            "agent",
            "--json",
        ],
    );
    assert!(ok, "{v}");
    assert_eq!(
        v["data"]["reviews"].as_array().unwrap(),
        &["PR-9"],
        "set-review replaces; add-review appends: {v}"
    );
}

#[test]
fn a_task_file_written_with_the_legacy_singular_review_id_still_loads() {
    // Guards the migration: task files predating the plural field must not become unreadable.
    let d = setup();
    let id = mk(d.path(), "legacy");
    let path = d
        .path()
        .join("workspaces/main/tasks")
        .join(&id)
        .join("task.md");
    let text = std::fs::read_to_string(&path).unwrap();
    let legacy = text.replace("reviews: []", "review_id: PR-legacy");
    assert_ne!(legacy, text, "the fixture actually rewrote the field");
    std::fs::write(&path, legacy).unwrap();

    let (ok, v) = run(d.path(), &["task", "show", "--id", &id, "--json"]);
    assert!(ok, "a legacy file still reads: {v}");
    assert_eq!(
        v["data"]["reviews"].as_array().unwrap(),
        &["PR-legacy"],
        "{v}"
    );
}

#[test]
fn the_new_capture_fields_are_audited_against_their_actor() {
    let d = setup();
    let id = mk(d.path(), "audited");
    for args in [
        vec!["set-kind", "--kind", "chore"],
        vec!["set-ticket", "--ticket", "T-1"],
        vec!["add-tag", "--tag", "x"],
        vec!["remove-tag", "--tag", "x"],
        vec!["add-review", "--review-id", "PR-1"],
        vec!["remove-review", "--review-id", "PR-1"],
    ] {
        let mut argv = vec!["task"];
        argv.extend(args);
        argv.extend(["--id", &id, "--actor", "aaron", "--json"]);
        run(d.path(), &argv);
    }
    let actions = audit_actions(d.path(), &id);
    for expected in [
        "set_kind",
        "set_ticket",
        "add_tag",
        "remove_tag",
        "add_review",
        "remove_review",
    ] {
        assert!(
            actions.iter().any(|a| a == expected),
            "{expected} missing from {actions:?}"
        );
    }
}

#[test]
fn the_new_setters_honour_the_version_guard() {
    let d = setup();
    let id = mk(d.path(), "contended");
    let (ok, v) = run(
        d.path(),
        &[
            "task",
            "set-kind",
            "--id",
            &id,
            "--kind",
            "bug",
            "--actor",
            "aaron",
            "--version",
            "99",
            "--json",
        ],
    );
    assert!(!ok, "{v}");
    assert_eq!(v["errors"][0]["code"], "CONFLICT_VERSION_MISMATCH", "{v}");
}

// ---------------------------------------------------------------------------
// S2 — staleness: surfacing work that stopped moving.
// ---------------------------------------------------------------------------

/// Rewrite a stored task's `updated_at`, standing in for the passage of time.
fn backdate(root: &std::path::Path, id: &str, updated_at: &str) {
    let path = root.join("workspaces/main/tasks").join(id).join("task.md");
    let text = std::fs::read_to_string(&path).unwrap();
    let out: Vec<String> = text
        .lines()
        .map(|l| {
            if l.starts_with("updated_at:") {
                format!("updated_at: {updated_at}")
            } else {
                l.to_string()
            }
        })
        .collect();
    std::fs::write(&path, out.join("\n")).unwrap();
}

fn ids(v: &serde_json::Value) -> Vec<String> {
    v["data"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|t| t["id"].as_str().unwrap_or_default().to_string())
        .collect()
}

#[test]
fn stale_lists_only_tasks_that_have_stopped_moving() {
    let d = setup();
    let old = mk(d.path(), "abandoned");
    let fresh = mk(d.path(), "active");
    run(
        d.path(),
        &["task", "start", "--id", &old, "--actor", "agent", "--json"],
    );
    run(
        d.path(),
        &[
            "task", "start", "--id", &fresh, "--actor", "agent", "--json",
        ],
    );
    backdate(d.path(), &old, "2026-01-01T00:00:00Z");

    let (ok, v) = run(d.path(), &["task", "list", "--stale", "7d", "--json"]);
    assert!(ok, "{v}");
    assert_eq!(ids(&v), vec![old], "only the abandoned one: {v}");
}

#[test]
fn stale_ignores_finished_work_however_old() {
    // A task done in January has not moved since, and that is correct rather than abandoned.
    let d = setup();
    let done = mk(d.path(), "finished long ago");
    run(
        d.path(),
        &["task", "start", "--id", &done, "--actor", "agent", "--json"],
    );
    run(
        d.path(),
        &[
            "task", "complete", "--id", &done, "--actor", "agent", "--json",
        ],
    );
    backdate(d.path(), &done, "2026-01-01T00:00:00Z");

    let (_, v) = run(d.path(), &["task", "list", "--stale", "7d", "--json"]);
    assert!(ids(&v).is_empty(), "{v}");
}

#[test]
fn a_merged_task_waiting_to_be_accepted_does_go_stale() {
    // The reason `merged` is not terminal: it is the state that rots while awaiting a human.
    let d = setup();
    let id = mk(d.path(), "awaiting acceptance");
    for s in ["start", "request-review", "merge"] {
        run(
            d.path(),
            &["task", s, "--id", &id, "--actor", "agent", "--json"],
        );
    }
    backdate(d.path(), &id, "2026-01-01T00:00:00Z");
    let (_, v) = run(d.path(), &["task", "list", "--stale", "7d", "--json"]);
    assert_eq!(ids(&v), vec![id], "{v}");
}

#[test]
fn overdue_lists_only_tasks_past_their_expected_by() {
    let d = setup();
    let late = mk(d.path(), "late");
    let ontime = mk(d.path(), "on time");
    for id in [&late, &ontime] {
        run(
            d.path(),
            &["task", "start", "--id", id, "--actor", "agent", "--json"],
        );
    }
    run(
        d.path(),
        &[
            "task",
            "set-review",
            "--id",
            &late,
            "--expected-by",
            "2026-01-01T00:00:00Z",
            "--actor",
            "agent",
            "--json",
        ],
    );
    run(
        d.path(),
        &[
            "task",
            "set-review",
            "--id",
            &ontime,
            "--expected-by",
            "2099-01-01T00:00:00Z",
            "--actor",
            "agent",
            "--json",
        ],
    );
    let (ok, v) = run(d.path(), &["task", "list", "--overdue", "--json"]);
    assert!(ok, "{v}");
    assert_eq!(ids(&v), vec![late], "{v}");
}

#[test]
fn a_task_with_no_expected_by_is_not_overdue() {
    let d = setup();
    let id = mk(d.path(), "no window");
    run(
        d.path(),
        &["task", "start", "--id", &id, "--actor", "agent", "--json"],
    );
    let (_, v) = run(d.path(), &["task", "list", "--overdue", "--json"]);
    assert!(ids(&v).is_empty(), "{v}");
}

#[test]
fn a_nonsense_stale_age_is_refused_rather_than_ignored() {
    let d = setup();
    let (ok, v) = run(
        d.path(),
        &["task", "list", "--stale", "yesterday", "--json"],
    );
    assert!(!ok, "{v}");
    assert_eq!(v["errors"][0]["code"], "INVALID_AGE", "{v}");
}

#[test]
fn stale_and_status_filters_compose() {
    let d = setup();
    let running = mk(d.path(), "running and old");
    let pending = mk(d.path(), "pending and old");
    run(
        d.path(),
        &[
            "task", "start", "--id", &running, "--actor", "agent", "--json",
        ],
    );
    run(
        d.path(),
        &[
            "task", "pending", "--id", &pending, "--actor", "agent", "--json",
        ],
    );
    for id in [&running, &pending] {
        backdate(d.path(), id, "2026-01-01T00:00:00Z");
    }
    let (_, v) = run(
        d.path(),
        &[
            "task", "list", "--stale", "7d", "--status", "running", "--json",
        ],
    );
    assert_eq!(ids(&v), vec![running], "{v}");
}

// ---------------------------------------------------------------------------
// OWNER_NOT_FOUND names the valid owners, so the fix travels with the error.
//
// The reason this matters: SKILL.md is the always-loaded file and reference.md is explicitly
// not loaded until needed, so an agent whose first action is `task create` can hit this with
// no owner-registry guidance in its context at all. Naming the registered owners in the
// message puts the answer where the caller already is — the same reasoning as INVALID_KIND
// listing the legal kinds.
// ---------------------------------------------------------------------------

/// The error message from a command expected to fail.
fn error_message(v: &serde_json::Value) -> String {
    v["errors"][0]["message"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn owner_not_found_names_the_registered_owners() {
    let d = setup(); // registers "agent"
    run(
        d.path(),
        &[
            "owner", "add", "--name", "aaron", "--type", "human", "--json",
        ],
    );
    let (ok, v) = run(
        d.path(),
        &[
            "task", "create", "--title", "x", "--owner", "nobody", "--actor", "agent", "--json",
        ],
    );
    assert!(!ok, "{v}");
    assert_eq!(v["errors"][0]["code"], "OWNER_NOT_FOUND");
    let msg = error_message(&v);
    assert!(msg.contains("nobody"), "names the bad owner: {msg}");
    for known in ["agent", "aaron"] {
        assert!(
            msg.contains(known),
            "message must name registered owner {known:?}: {msg}"
        );
    }
}

#[test]
fn owner_not_found_on_an_empty_registry_says_how_to_add_one() {
    // "registered owners are: " with nothing after it would be worse than useless, and this is
    // the state a brand-new store is in — the most likely moment to hit this error.
    let d = tempfile::tempdir().unwrap();
    run(d.path(), &["init", "--json"]);
    let (ok, v) = run(
        d.path(),
        &[
            "task", "create", "--title", "x", "--owner", "nobody", "--actor", "me", "--json",
        ],
    );
    assert!(!ok, "{v}");
    let msg = error_message(&v);
    assert!(
        msg.contains("owner add"),
        "an empty registry gets the command that fixes it: {msg}"
    );
}

#[test]
fn assign_and_set_reviewer_name_the_registered_owners_too() {
    // All three sites that validate an owner share one message, so none of them can drift.
    let d = setup();
    let id = mk(d.path(), "t");
    for (cmd, flag) in [("assign", "--owner"), ("set-reviewer", "--reviewer")] {
        let (ok, v) = run(
            d.path(),
            &[
                "task", cmd, "--id", &id, flag, "nobody", "--actor", "agent", "--json",
            ],
        );
        assert!(!ok, "{cmd}: {v}");
        assert_eq!(v["errors"][0]["code"], "OWNER_NOT_FOUND", "{cmd}");
        let msg = error_message(&v);
        assert!(
            msg.contains("agent"),
            "{cmd} must name the registered owners: {msg}"
        );
    }
}

#[test]
fn a_date_only_expected_by_is_accepted_and_stored() {
    // The lenient form callers already use. Validation must not become a wall in front of it.
    let d = setup();
    let id = mk(d.path(), "t");
    let (ok, v) = run(
        d.path(),
        &[
            "task",
            "set-review",
            "--id",
            &id,
            "--expected-by",
            "2026-09-08",
            "--actor",
            "agent",
            "--json",
        ],
    );
    assert!(ok, "a bare date is a legal expected-by: {v}");
    assert_eq!(v["data"]["expected_by"], "2026-09-08");
}

#[test]
fn an_unreadable_date_is_refused_by_every_command_that_takes_one() {
    // One shared validator, so no date-taking flag can drift back to storing anything at all.
    // The failure this prevents: a value written here that no reader can parse, which makes the
    // task silently invisible to `--overdue` rather than loudly wrong.
    let d = setup();
    for (cmd, flag) in [("set-review", "--expected-by"), ("set-due", "--due-at")] {
        let id = mk(d.path(), "t");
        let (ok, v) = run(
            d.path(),
            &[
                "task",
                cmd,
                "--id",
                &id,
                flag,
                "next tuesday",
                "--actor",
                "agent",
                "--json",
            ],
        );
        assert!(!ok, "{cmd} must refuse an unreadable date: {v}");
        assert_eq!(v["errors"][0]["code"], "INVALID_DATE", "{cmd}");
        let msg = error_message(&v);
        assert!(
            msg.contains("2026-09-08") || msg.contains("YYYY-MM-DD"),
            "{cmd} must show the legal form: {msg}"
        );
    }
}

#[test]
fn a_refused_date_leaves_the_task_untouched() {
    // Validated before anything is written, the same rule `--kind` follows: a bad value creates
    // no half-applied task.
    let d = setup();
    let id = mk(d.path(), "t");
    run(
        d.path(),
        &[
            "task",
            "set-review",
            "--id",
            &id,
            "--review-id",
            "CR-1",
            "--expected-by",
            "2026-09-08",
            "--actor",
            "agent",
            "--json",
        ],
    );
    let (_, before) = run(d.path(), &["task", "show", "--id", &id, "--json"]);
    run(
        d.path(),
        &[
            "task",
            "set-review",
            "--id",
            &id,
            "--review-id",
            "CR-2",
            "--expected-by",
            "garbage",
            "--actor",
            "agent",
            "--json",
        ],
    );
    let (_, after) = run(d.path(), &["task", "show", "--id", &id, "--json"]);
    assert_eq!(
        before["data"]["reviews"], after["data"]["reviews"],
        "the review must not be replaced when the date alongside it is refused"
    );
    assert_eq!(before["data"]["version"], after["data"]["version"]);
}

/// Raw runner for output that is not the JSON envelope — `--version` is clap's, not ours.
fn run_raw(args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_taskforge"))
        .args(args)
        .output()
        .expect("binary runs");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).to_string(),
    )
}

#[test]
fn version_names_the_commit_it_was_built_from() {
    // A bare semver cannot distinguish an installed binary from repo HEAD, which is how a week's
    // worth of committed features stayed unreachable without anything reporting it.
    let (ok, out) = run_raw(&["--version"]);
    assert!(ok, "--version succeeds: {out}");
    let commit = out
        .split('(')
        .nth(1)
        .and_then(|s| s.split(')').next())
        .unwrap_or_default()
        .to_string();
    assert_eq!(commit.len(), 7, "a 7-char short sha, got {out:?}");
    assert!(
        commit.chars().all(|c| c.is_ascii_hexdigit()),
        "the parenthesised part must be a sha: {out:?}"
    );
}

#[test]
fn the_envelope_version_stays_a_bare_semver() {
    // `taskforge_version` is a published contract. An agent comparing it against a number should
    // not have to strip a commit suffix, so the commit goes in `--version` and `doctor` only.
    let d = setup();
    let (_, v) = run(d.path(), &["task", "list", "--json"]);
    let ver = v["taskforge_version"].as_str().unwrap();
    assert!(
        !ver.contains('(') && ver.split('.').count() == 3,
        "expected a bare x.y.z, got {ver:?}"
    );
}

#[test]
fn doctor_reports_this_test_binary_as_fresh() {
    // Built from the checkout it is now comparing against, so the only honest answer is fresh.
    // The stale verdict cannot be produced this way — a test cannot build a binary from a commit
    // that does not exist yet — which is why `freshness` is a pure function tested separately.
    let d = setup();
    let (ok, v) = run(d.path(), &["doctor", "--json"]);
    assert!(ok, "fresh must exit 0: {v}");
    assert_eq!(v["data"]["state"], "fresh", "{v}");
    assert_eq!(
        v["data"]["binary"]["commit"], v["data"]["source"]["commit"],
        "fresh means the two commits agree: {v}"
    );
}

#[test]
fn doctor_reports_where_the_source_is_and_when_the_binary_was_built() {
    // Without these a stale verdict is a dead end: you know something is wrong and not which
    // checkout to reinstall from.
    let d = setup();
    let (_, v) = run(d.path(), &["doctor", "--json"]);
    assert!(
        v["data"]["source"]["dir"].as_str().unwrap().contains('/'),
        "a real path: {v}"
    );
    assert!(
        v["data"]["binary"]["built_at"]
            .as_str()
            .unwrap()
            .starts_with("20"),
        "an RFC3339 timestamp: {v}"
    );
    assert_eq!(
        v["data"]["source"]["version"], v["data"]["binary"]["version"],
        "a fresh build's two versions agree: {v}"
    );
}
