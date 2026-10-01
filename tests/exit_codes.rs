//! Every documented exit code, and the shape of the error envelope.

mod common;

use common::{Env, Mock};

#[test]
fn zero_ok_including_help_and_version() {
    let env = Env::new();
    assert!(env.cmd().arg("--help").output().unwrap().status.success());
    assert!(env.cmd().arg("--version").output().unwrap().status.success());
    env.ok(&["init"]);
    assert!(env.cmd().args(["job", "list"]).output().unwrap().status.success());
}

#[test]
fn one_internal_on_a_corrupt_database() {
    let env = Env::new();
    env.ok(&["init"]);
    // Replace the database with something that is not SQLite.
    for ext in ["open-apply.db-wal", "open-apply.db-shm"] {
        let _ = std::fs::remove_file(env.home.join(ext));
    }
    std::fs::write(env.home.join("open-apply.db"), "this is not a database, not even close, ".repeat(50)).unwrap();
    let err = env.fails(1, &["job", "list"]);
    assert_eq!(err["error"]["code"], "internal");
}

#[test]
fn two_usage_for_bad_flags_and_missing_arguments() {
    let env = Env::new();
    let err = env.fails(2, &["job", "add"]);
    assert_eq!(err["error"]["code"], "usage");
    assert!(err["error"]["hint"].as_str().unwrap().contains("Usage:"));
    env.fails(2, &["no-such-command"]);
    env.fails(2, &["stale", "--days", "soon"]);
    // Bare invocation prints help and is also a usage error.
    assert_eq!(env.cmd().output().unwrap().status.code(), Some(2));
}

#[test]
fn three_not_found_for_unknown_ids_and_uninitialized_home() {
    let env = Env::new();
    let err = env.fails(3, &["job", "list"]);
    assert_eq!(err["error"]["code"], "not_found");
    assert!(err["error"]["hint"].as_str().unwrap().contains("open-apply init"));
    env.init();
    env.fails(3, &["job", "show", "oa_deadbeef"]);
    env.fails(3, &["prepare", "oa_deadbeef"]);
    env.fails(3, &["applied", "oa_deadbeef"]);
    env.fails(3, &["source", "remove", "greenhouse:nothing"]);
    env.fails(3, &["import", env.dir.path().join("missing.json").to_str().unwrap()]);
}

#[test]
fn four_validation_for_rejected_input() {
    let env = Env::new();
    env.init();
    env.fails(4, &["profile", "set", "shoe_size", "42"]);
    env.fails(4, &["profile", "set", "email", "not-an-email"]);
    env.fails(4, &["source", "add", "linkedin:acme"]);
    env.fails(4, &["source", "add", "greenhouse"]);
    env.fails(4, &["job", "add", "ftp://example.com/x", "--title", "T", "--no-fetch"]);
    env.fails(4, &["job", "add", "https://example.com/x", "--no-fetch"]); // no title
    let id = env.add_job("https://example.com/jobs/1", "Acme", "Engineer");
    env.fails(4, &["status", &id, "hired"]);
    env.fails(4, &["status", &id, "applied"]);
    env.fails(4, &["event", "add", &id, "--type", "created"]);
    env.fails(4, &["applied", &id, "--at", "someday"]);
    env.fails(4, &["applied", &id, "--at", "2027-01-01"]);
    env.fails(4, &["applied", &id, "--via", "carrier-pigeon"]);
    env.fails(4, &["job", "rm", &id]); // needs --yes
    env.fails(4, &["export", "--format", "xml"]);
    env.fails(4, &["search"]); // no sources watched and none given
    env.fails(4, &["materials", "attach", &id, "--cv", env.dir.path().join("nope.pdf").to_str().unwrap()]);
    // doctor reports failure with exit 4 and still prints the report.
    let fresh = Env::new();
    fresh.ok(&["init"]);
    let out = fresh.cmd().args(["--json", "doctor"]).output().unwrap();
    assert_eq!(out.status.code(), Some(4));
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["ok"], false);
    assert!(report["checks"].as_array().unwrap().iter().any(|c| c["check"] == "profile" && c["status"] == "fail"));
}

#[test]
fn five_network_when_the_upstream_is_down_or_refuses() {
    // Nothing listens on the default dead URL.
    let env = Env::new();
    env.init();
    let err = env.fails(5, &["search", "--source", "remoteok"]);
    assert_eq!(err["error"]["code"], "network");
    let err = env.fails(5, &["job", "add", "https://boards.greenhouse.io/acme-labs/jobs/4012345"]);
    assert_eq!(err["error"]["code"], "network");
    assert!(err["error"]["hint"].as_str().unwrap().contains("--no-fetch"));

    // A server that answers 403: open-apply reports it and does not try to work around it.
    let mock = Mock::start(vec![
        ("/api", 403, "text/html", "blocked".into()),
        ("/api/job-board-api", 200, "application/json", "<html>oops</html>".into()),
    ]);
    let env = Env::with_mock(&mock);
    env.init();
    env.fails(5, &["search", "--source", "remoteok"]);
    // A 200 with a body that is not the documented shape is an upstream problem too.
    env.fails(5, &["search", "--source", "arbeitnow"]);
    assert_eq!(mock.requests().iter().filter(|r| r.path == "/api").count(), 1, "no retries after a refusal");
}

#[test]
fn six_guardrail_when_the_daily_cap_is_hit() {
    let env = Env::new();
    env.init();
    env.write_config("daily_application_cap: 1\n");
    let a = env.add_job("https://a.example/jobs/1", "Acme", "Engineer");
    let b = env.add_job("https://b.example/jobs/1", "Globex", "Engineer");
    env.ok(&["applied", &a]);
    let err = env.fails(6, &["applied", &b]);
    assert_eq!(err["error"]["code"], "guardrail");
    assert!(err["error"]["hint"].as_str().unwrap().contains("--force"));
    assert_eq!(err["error"]["detail"]["violations"][0]["guardrail"], "daily_cap");
}

#[test]
fn seven_conflict_for_duplicates() {
    let env = Env::new();
    env.init();
    let id = env.add_job("https://example.com/jobs/1?utm_source=newsletter", "Acme", "Engineer");
    let err = env.fails(7, &["job", "add", "https://www.example.com/jobs/1/#apply", "--title", "T", "--no-fetch"]);
    assert_eq!(err["error"]["code"], "conflict");
    assert_eq!(err["error"]["detail"]["existing_id"], id.as_str());
    assert_eq!(err["error"]["detail"]["matched_by"], "url");
}

#[test]
fn error_envelope_shape_json_and_yaml() {
    let env = Env::new();
    env.init();
    let json = env.fails(3, &["job", "show", "oa_deadbeef"]);
    let e = json["error"].as_object().unwrap();
    assert_eq!(e["code"], "not_found");
    assert!(e["message"].as_str().unwrap().contains("oa_deadbeef"));
    assert!(e["hint"].is_string());
    // Default stderr is YAML with the same keys.
    let out = env.cmd().args(["job", "show", "oa_deadbeef"]).output().unwrap();
    let text = String::from_utf8(out.stderr).unwrap();
    assert!(text.starts_with("error:\n  code: not_found\n  message: "), "{text}");
    assert!(text.contains("\n  hint: "));
    assert!(out.stdout.is_empty());
}
