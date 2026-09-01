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
