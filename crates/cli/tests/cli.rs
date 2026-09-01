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
fn review_id_expected_by_and_worker_are_settable_and_persist() {
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
            "CR-301625168",
            "--expected-by",
            "2026-09-03T17:00:00Z",
            "--worker",
            "addresscr-CR-301625168",
            "--actor",
            "agent",
            "--json",
        ],
    );
    assert!(ok, "set-review succeeds");

    let (_, shown) = run(d.path(), &["task", "show", "--id", &id, "--json"]);
    assert_eq!(shown["data"]["review_id"], "CR-301625168");
    assert_eq!(shown["data"]["expected_by"], "2026-09-03T17:00:00Z");
    assert_eq!(shown["data"]["worker"], "addresscr-CR-301625168");
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
