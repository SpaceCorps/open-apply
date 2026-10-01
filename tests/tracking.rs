//! Day-to-day tracking: status, events, follow-ups, stats filters, job edits, export formats.

mod common;

use common::Env;

fn setup_applied(env: &Env, url: &str, company: &str, at: &str) -> String {
    let id = env.add_job(url, company, "Engineer");
    env.ok(&["applied", &id, "--at", at, "--force"]);
    id
}

#[test]
fn status_changes_append_events_and_map_to_event_types() {
    let env = Env::new();
    env.init();
    let id = env.add_job("https://a.example/jobs/1", "Acme", "Engineer");
    let v = env.ok(&["status", &id, "ready", "--note", "cv tailored"]);
    assert_eq!((v["from"].as_str(), v["to"].as_str()), (Some("saved"), Some("ready")));
    env.fails(7, &["status", &id, "ready"]); // no-op changes are flagged
    env.ok(&["applied", &id, "--at", "2026-09-20"]);
    env.ok(&["status", &id, "interview", "--at", "2026-09-29"]);
    env.ok(&["status", &id, "withdrawn", "--note", "took another offer"]);
    let shown = env.ok(&["job", "show", &id]);
    assert_eq!(shown["status"], "withdrawn");
    let types: Vec<&str> = shown["events"].as_array().unwrap().iter().map(|e| e["type"].as_str().unwrap()).collect();
    assert_eq!(types, ["created", "applied", "interview", "status", "status"]); // creation first, then by date
    // A manual move to interview counts as a response in the stats.
    assert_eq!(env.ok(&["stats"])["responded"], 1);
}

#[test]
fn event_add_moves_forward_only_and_records_everything() {
    let env = Env::new();
    env.init();
    let id = setup_applied(&env, "https://a.example/jobs/1", "Acme", "2026-09-01");
    let ack = env.ok(&["event", "add", &id, "--type", "ack", "--at", "2026-09-02"]);
    assert_eq!((ack["status_before"].as_str(), ack["status"].as_str()), (Some("applied"), Some("acknowledged")));
    env.ok(&["event", "add", &id, "--type", "assessment", "--at", "2026-09-05"]);
    env.ok(&["event", "add", &id, "--type", "interview", "--at", "2026-09-10"]);
    let late = env.ok(&[
        "event",
        "add",
        &id,
        "--type",
        "screen",
        "--at",
        "2026-09-11",
        "--note",
        "recruiter call, logged late",
    ]);
    assert_eq!(late["status"], "interview");
    assert_eq!(late["status_changed"], false);
    let note = env.ok(&["event", "add", &id, "--type", "note", "--note", "sent thank-you email"]);
    assert_eq!(note["status_changed"], false);
    let shown = env.ok(&["job", "show", &id]);
    assert_eq!(shown["events"].as_array().unwrap().len(), 7);
    // Rejection ends it; later events do not resurrect it.
    env.ok(&["event", "add", &id, "--type", "rejection", "--at", "2026-09-20"]);
    assert_eq!(env.ok(&["event", "add", &id, "--type", "offer", "--at", "2026-09-21"])["status"], "rejected");
}

#[test]
fn followups_respect_idle_time_and_the_follow_up_limit() {
    let env = Env::new();
    env.init(); // max_followups: 2
    let fresh = setup_applied(&env, "https://a.example/jobs/1", "Acme", "2026-09-28"); // 3 days
    let due = setup_applied(&env, "https://b.example/jobs/1", "Globex", "2026-09-20"); // 11 days
    let old = setup_applied(&env, "https://c.example/jobs/1", "Initech", "2026-09-01"); // 30 days

    let v = env.ok(&["followups"]);
    let ids: Vec<&str> = v["jobs"].as_array().unwrap().iter().map(|j| j["id"].as_str().unwrap()).collect();
    assert_eq!(ids, [old.as_str(), due.as_str()], "oldest first, fresh application left alone");
    assert!(!ids.contains(&fresh.as_str()));
    assert_eq!(v["jobs"][0]["days_since_activity"], 30);
    assert!(v["hint"].as_str().unwrap().contains("follow_up"));

    // Recording a follow-up resets the clock...
    env.ok(&["event", "add", &due, "--type", "follow_up", "--at", "2026-09-29"]);
    assert_eq!(env.ok(&["followups"])["count"], 1);
    // ...a narrower window brings it back...
    assert_eq!(env.ok(&["followups", "--days", "2"])["count"], 3);
    // ...and after two follow-ups the tool stops suggesting more.
    env.ok(&["event", "add", &old, "--type", "follow_up", "--at", "2026-09-10"]);
    env.ok(&["event", "add", &old, "--type", "follow_up", "--at", "2026-09-17"]);
    let ids: Vec<String> = env.ok(&["followups"])["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| j["id"].as_str().unwrap().to_string())
        .collect();
    assert!(!ids.contains(&old));
}

#[test]
fn stale_ignores_answered_and_recent_applications() {
    let env = Env::new();
    env.init();
    let silent = setup_applied(&env, "https://a.example/jobs/1", "Acme", "2026-08-15");
    let answered = setup_applied(&env, "https://b.example/jobs/1", "Globex", "2026-08-15");
    setup_applied(&env, "https://c.example/jobs/1", "Initech", "2026-09-25");
    env.ok(&["event", "add", &answered, "--type", "ack", "--at", "2026-08-16"]);
    let v = env.ok(&["stale"]);
    assert_eq!(v["count"], 1);
    assert_eq!(v["jobs"][0]["id"], silent.as_str());
    assert!(v["hint"].as_str().unwrap().contains("--mark"));
    // A stricter window catches the recent one too.
    assert_eq!(env.ok(&["stale", "--days", "5"])["count"], 2);
}

#[test]
fn stats_since_and_empty_database() {
    let env = Env::new();
    env.init();
    let empty = env.ok(&["stats"]);
    assert_eq!(empty["applied"], 0);
    assert!(empty.get("response_rate").is_none(), "no applications means no rate, not 0%");
    let a = setup_applied(&env, "https://a.example/jobs/1", "Acme", "2026-07-01");
    let b = setup_applied(&env, "https://b.example/jobs/1", "Globex", "2026-09-20");
    env.ok(&["event", "add", &a, "--type", "rejection", "--at", "2026-07-11"]);
    env.ok(&["event", "add", &b, "--type", "ack", "--at", "2026-09-22"]);
    let all = env.ok(&["stats"]);
    assert_eq!(all["applied"], 2);
    assert_eq!(all["response_rate"], 1.0);
    assert_eq!(all["median_days_to_first_response"], 6.0); // (10 + 2) / 2
    let recent = env.ok(&["stats", "--since", "2026-09-01"]);
    assert_eq!(recent["applied"], 1);
    assert_eq!(recent["median_days_to_first_response"], 2.0);
    assert_eq!(recent["funnel"]["acknowledged"], 1);
    assert_eq!(recent["funnel"]["rejected"], 0);
    env.fails(4, &["stats", "--since", "last tuesday"]);
}

#[test]
fn job_list_filters() {
    let env = Env::new();
    env.init();
    let a = env.add_job("https://a.example/jobs/1", "Acme Corp", "Engineer");
    env.add_job("https://b.example/jobs/1", "Globex", "Engineer");
    env.add_job("https://c.example/jobs/1", "Acme Labs", "Designer");
    env.ok(&["applied", &a]);
    assert_eq!(env.ok(&["job", "list"])["count"], 3);
    assert_eq!(env.ok(&["job", "list", "--company", "acme"])["count"], 2);
    assert_eq!(env.ok(&["job", "list", "--status", "applied"])["count"], 1);
    assert_eq!(env.ok(&["job", "list", "--status", "applied,saved"])["count"], 3);
    assert_eq!(env.ok(&["job", "list", "--limit", "1"])["count"], 1);
    assert_eq!(env.ok(&["job", "list", "--since", "2026-10-02"])["count"], 0);
    assert_eq!(env.ok(&["job", "list", "--since", "2026-09-30"])["count"], 3);
    env.fails(4, &["job", "list", "--status", "hired"]);
    env.fails(4, &["job", "list", "--limit", "0"]);
}

#[test]
fn job_update_and_rm() {
    let env = Env::new();
    env.init();
    let id = env.add_job("https://a.example/jobs/1", "Acme", "Engineer");
    env.fails(4, &["job", "update", &id]);
    env.fails(4, &["job", "update", &id, "--applied-at", "2026-09-01"]); // not applied yet
    let v = env.ok(&[
        "job",
        "update",
        &id,
        "--title",
        "Staff Engineer",
        "--location",
        "Oslo",
        "--notes",
        "warm intro from Sam",
    ]);
    assert_eq!(v["job"]["title"], "Staff Engineer");
    assert_eq!(env.ok(&["job", "show", &id])["notes"], "warm intro from Sam");

    env.ok(&["applied", &id, "--at", "2026-09-10"]);
    let fixed = env.ok(&["job", "update", &id, "--applied-at", "2026-09-08", "--via", "email"]);
    assert_eq!(fixed["job"]["applied_at"], "2026-09-08T00:00:00Z");
    let shown = env.ok(&["job", "show", &id]);
    let last = shown["events"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["type"], "note");
    assert!(last["note"].as_str().unwrap().contains("applied_at 2026-09-10T00:00:00Z -> 2026-09-08T00:00:00Z"));

    env.ok(&["prepare", &id]);
    let gone = env.ok(&["job", "rm", &id, "--yes"]);
    assert_eq!(gone["deleted"], id.as_str());
    assert!(gone["workspace_left_in_place"].is_string(), "user files are never deleted");
    env.fails(3, &["job", "show", &id]);
    assert_eq!(env.ok(&["stats"])["applied"], 0);
}

#[test]
fn export_formats_and_csv_import() {
    let env = Env::new();
    env.init();
    let a = env.add_job("https://a.example/jobs/1", "Acme, Inc.", "Engineer \"Senior\"");
    env.ok(&["applied", &a, "--at", "2026-09-01", "--via", "referral"]);

    let csv = env.cmd().args(["export", "--format", "csv"]).output().unwrap();
    let csv = String::from_utf8(csv.stdout).unwrap();
    assert!(csv.starts_with("id,status,title,company,location,source,url,applied_via,applied_at,"));
    assert!(csv.contains("\"Engineer \"\"Senior\"\"\",\"Acme, Inc.\""), "{csv}");
    assert!(csv.contains(",referral,2026-09-01T00:00:00Z,"));

    let md = env.cmd().args(["export", "--format", "md"]).output().unwrap();
    let md = String::from_utf8(md.stdout).unwrap();
    assert!(md.contains("| id | status | company | title |"));
    assert!(md.contains("| applied | Acme, Inc. |"));

    let json = env.cmd().args(["export"]).output().unwrap();
    let json: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(json["format"], "open-apply-export");
    assert_eq!(json["jobs"][0]["events"].as_array().unwrap().len(), 2);

    // A spreadsheet of past applications can be brought in.
    let sheet = env.dir.path().join("past.csv");
    std::fs::write(&sheet, "Title,Company,URL,Applied_At,Status\nBackend Dev,Hooli,https://hooli.example/j/1,2026-08-20,rejected\nFrontend Dev,Pied Piper,https://piedpiper.example/j/2,2026-09-02,\n,,,,\n").unwrap();
    let report = env.ok(&["import", sheet.to_str().unwrap()]);
    assert_eq!(report["kind"], "csv");
    assert_eq!(report["imported"], 2);
    assert_eq!(report["skipped"].as_array().unwrap().len(), 0);
    let jobs = env.ok(&["job", "list", "--company", "hooli"]);
    assert_eq!(jobs["jobs"][0]["status"], "rejected");
    assert_eq!(env.ok(&["stats"])["applied"], 3);
    assert_eq!(env.ok(&["import", sheet.to_str().unwrap()])["skipped_existing"], 2);
}

#[test]
fn export_to_file_reports_counts() {
    let env = Env::new();
    env.init();
    env.add_job("https://a.example/jobs/1", "Acme", "Engineer");
    let path = env.dir.path().join("out").join("jobs.csv");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let v = env.ok(&["export", "--format", "csv", "--out", path.to_str().unwrap()]);
    assert_eq!((v["jobs"].as_u64(), v["format"].as_str()), (Some(1), Some("csv")));
    assert!(std::fs::read_to_string(&path).unwrap().contains("Acme"));
}
