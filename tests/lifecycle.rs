//! The full path a job takes: init -> job add -> prepare -> applied -> triage -> stats -> stale ->
//! export/import. Runs the real binary against a temp home and an in-process mock of the ATS.

mod common;

use common::{Env, Mock};

#[test]
fn full_lifecycle() {
    let mock = Mock::feeds();
    let env = Env::with_mock(&mock);

    // init: home, database, config, profile template.
    let init = env.ok(&["init"]);
    assert_eq!(init["schema_version"], 1);
    assert!(env.home.join("open-apply.db").exists());
    assert!(env.home.join("config.yaml").exists());
    assert!(env.home.join("profile.yaml").exists());
    assert_eq!(init["profile_missing"], serde_json::json!(["name", "email"]));
    // Running it again is harmless.
    let again = env.ok(&["init"]);
    assert_eq!(again["created"], serde_json::json!([]));

    env.ok(&["profile", "set", "name", "Ada Lovelace"]);
    env.ok(&["profile", "set", "email", "ada@example.com"]);
    env.ok(&["profile", "set", "location", "Helsinki"]);
    env.ok(&["profile", "set", "answers.why_us", "I want to work on {title} at {company}."]);
    let doctor = env.ok(&["doctor"]);
    assert_eq!(doctor["ok"], true);

    // job add: a Greenhouse URL is read through the public JSON endpoint.
    let added = env.ok(&["job", "add", "https://boards.greenhouse.io/acme-labs/jobs/4012345?gh_src=abc"]);
    let id = added["job"]["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("oa_") && id.len() == 11, "{id}");
    assert_eq!(added["job"]["title"], "Senior Rust Engineer");
    assert_eq!(added["job"]["company"], "Acme Labs");
    assert_eq!(added["job"]["status"], "saved");
    assert_eq!(added["job"]["source"], "greenhouse:acme-labs");
    assert_eq!(added["fetched"], true);
    let reqs = mock.requests();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].path, "/v1/boards/acme-labs/jobs/4012345");
    assert!(reqs[0].user_agent.starts_with("open-apply/"), "{}", reqs[0].user_agent);
    assert!(reqs[0].user_agent.contains("https://github.com/SpaceCorps/open-apply"));

    // prepare: workspace with the job description inside untrusted delimiters.
    let prepared = env.ok(&["prepare", &id]);
    assert_eq!(prepared["status"], "ready");
    let ws = env.home.join("workspaces").join(&id);
    assert_eq!(prepared["workspace"].as_str().unwrap().replace('\\', "/"), ws.to_string_lossy().replace('\\', "/"));
    let job_md = std::fs::read_to_string(ws.join("job.md")).unwrap();
    assert!(job_md.contains("--- untrusted job content begins ---"));
    assert!(job_md.contains("--- untrusted job content ends ---"));
    assert!(job_md.contains("Design the sync engine"));
    assert!(job_md.find("untrusted job content begins").unwrap() < job_md.find("Design the sync engine").unwrap());
    let profile_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.join("profile.json")).unwrap()).unwrap();
    assert_eq!(profile_json["profile"]["name"], "Ada Lovelace");
    let answers: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.join("answers.json")).unwrap()).unwrap();
    assert_eq!(answers["answers"]["why_us"], "I want to work on Senior Rust Engineer at Acme Labs.");
    assert_eq!(std::fs::read_to_string(ws.join("cover-letter.md")).unwrap(), "");

    // Preparing again keeps the cover letter the agent wrote.
    std::fs::write(ws.join("cover-letter.md"), "Dear Acme Labs,").unwrap();
    env.ok(&["prepare", &id]);
    assert_eq!(std::fs::read_to_string(ws.join("cover-letter.md")).unwrap(), "Dear Acme Labs,");

    // materials attach, then the work queue shows everything an agent needs.
    let cv = env.dir.path().join("cv.pdf");
    std::fs::write(&cv, "%PDF-1.4 fake").unwrap();
    env.ok(&["materials", "attach", &id, "--cv", cv.to_str().unwrap()]);
    let next = env.ok(&["next"]);
    assert_eq!(next["count"], 1);
    let item = &next["jobs"][0];
    assert_eq!(item["id"], id.as_str());
    assert_eq!(item["profile"]["email"], "ada@example.com");
    assert_eq!(item["materials"]["cv_exists"], true);
    assert_eq!(item["answers"]["why_us"], "I want to work on Senior Rust Engineer at Acme Labs.");
    assert_eq!(item["apply_via"], "ats");
    assert!(item["description"].as_str().unwrap().starts_with("--- untrusted job content begins ---"));
    assert_eq!(next["guardrails"]["remaining_today"], 25);

    // applied
    let applied = env.ok(&["applied", &id, "--at", "2026-09-10", "--note", "via careers page"]);
    assert_eq!(applied["status"], "applied");
    assert_eq!(applied["via"], "ats");
    assert_eq!(applied["applied_at"], "2026-09-10T00:00:00Z");
    // Recording it twice is a conflict, not a silent overwrite.
    env.fails(7, &["applied", &id]);

    // triage --apply: an automated acknowledgement from the ATS relay.
    let ack = env
        .cmd()
        .args(["--json", "triage", "--from", "Acme Labs <no-reply@us.greenhouse-mail.io>"])
        .args(["--subject", "Thank you for applying to Acme Labs", "--stdin", "--apply", "--at", "2026-09-11"])
        .write_stdin("Hi Ada, we have received your application for Senior Rust Engineer.")
        .output()
        .unwrap();
    assert!(ack.status.success(), "{}", String::from_utf8_lossy(&ack.stderr));
    let ack: serde_json::Value = serde_json::from_slice(&ack.stdout).unwrap();
    assert_eq!(ack["classification"]["class"], "ack");
    assert_eq!(ack["applied"], true);
    assert_eq!(ack["recorded"]["job_id"], id.as_str());
    assert_eq!(ack["recorded"]["status"], "acknowledged");

    // A screening invitation three days later.
    let screen = env
        .cmd()
        .args([
            "--json",
            "triage",
            "--from",
            "Jane <jane@acme-labs.com>",
            "--subject",
            "Intro call for Senior Rust Engineer?",
        ])
        .args(["--stdin", "--apply", "--at", "2026-09-14"])
        .write_stdin("Hi Ada, I would love to set up a 30 minute intro call to chat about the role.")
        .output()
        .unwrap();
    assert!(screen.status.success(), "{}", String::from_utf8_lossy(&screen.stderr));
    let screen: serde_json::Value = serde_json::from_slice(&screen.stdout).unwrap();
    assert_eq!(screen["classification"]["class"], "screen");
    assert_eq!(screen["recorded"]["status"], "screening");

    // job show has the full immutable history: the creation record first, then by date.
    let shown = env.ok(&["job", "show", &id]);
    let types: Vec<&str> = shown["events"].as_array().unwrap().iter().map(|e| e["type"].as_str().unwrap()).collect();
    assert_eq!(types, ["created", "applied", "ack", "screen", "status"]);
    assert_eq!(shown["status"], "screening");
    assert!(shown["description"].as_str().unwrap().starts_with("--- untrusted job content begins ---"));
    let sources: Vec<&str> =
        shown["events"].as_array().unwrap().iter().map(|e| e["source"].as_str().unwrap()).collect();
    assert_eq!(sources, ["cli", "cli", "triage", "triage", "cli"]);

    // A second application, long ago and never answered.
    let old = env.add_job("https://globex.example/careers/7", "Globex", "Support Engineer");
    env.ok(&["applied", &old, "--at", "2026-08-01", "--via", "email"]);

    // stats
    let stats = env.ok(&["stats"]);
    assert_eq!(stats["applied"], 2);
    assert_eq!(stats["responded"], 1);
    assert_eq!(stats["response_rate"], 0.5);
    assert_eq!(stats["median_days_to_first_response"], 1.0);
    assert_eq!(stats["funnel"]["screening"], 1);
    assert_eq!(stats["funnel"]["applied"], 1);
    assert_eq!(stats["ever_reached"]["screening"], 1);
    let by_via = stats["by_via"].as_array().unwrap();
    assert!(by_via.iter().any(|v| v["key"] == "email" && v["responded"] == 0));
    assert!(by_via.iter().any(|v| v["key"] == "ats" && v["responded"] == 1));
    let by_source = stats["by_source"].as_array().unwrap();
    assert!(by_source.iter().any(|v| v["key"] == "greenhouse" && v["response_rate"] == 1.0));

    // stale: Globex has been silent for 61 days.
    let stale = env.ok(&["stale", "--days", "21"]);
    assert_eq!(stale["count"], 1);
    assert_eq!(stale["jobs"][0]["id"], old.as_str());
    assert_eq!(stale["jobs"][0]["days_waiting"], 61);
    assert_eq!(env.ok(&["job", "show", &old])["status"], "applied", "listing alone changes nothing");
    let marked = env.ok(&["stale", "--days", "21", "--mark"]);
    assert_eq!(marked["marked"], 1);
    assert_eq!(env.ok(&["job", "show", &old])["status"], "ghosted");
    assert_eq!(env.ok(&["stale", "--days", "21"])["count"], 0);

    // A late reply still moves a ghosted application forward.
    let late = env.ok(&["event", "add", &old, "--type", "ack", "--at", "2026-09-20"]);
    assert_eq!(late["status"], "acknowledged");

    // export -> import into a fresh home keeps jobs and history.
    let export_path = env.dir.path().join("export.json");
    let exported = env.ok(&["export", "--format", "json", "--out", export_path.to_str().unwrap()]);
    assert_eq!(exported["jobs"], 2);

    let other = Env::new();
    other.init();
    let imported = other.ok(&["import", export_path.to_str().unwrap()]);
    assert_eq!(imported["imported"], 2);
    assert_eq!(imported["skipped_existing"], 0);
    let shown2 = other.ok(&["job", "show", &id]);
    assert_eq!(shown2["status"], "screening");
    assert_eq!(shown2["events"].as_array().unwrap().len(), 5);
    assert_eq!(other.ok(&["stats"])["applied"], 2);
    assert_eq!(other.ok(&["import", export_path.to_str().unwrap()])["imported"], 0);
}

#[test]
fn output_is_yaml_by_default_and_json_on_request() {
    let env = Env::new();
    env.init();
    let yaml = env.cmd().args(["job", "list"]).output().unwrap();
    assert!(yaml.status.success());
    let text = String::from_utf8(yaml.stdout).unwrap();
    assert!(text.starts_with("count: 0"), "{text}");
    assert!(serde_json::from_str::<serde_json::Value>(&text).is_err());
    let json = env.ok(&["job", "list"]);
    assert_eq!(json["count"], 0);
}

#[test]
fn stderr_holds_only_the_error_envelope_when_captured() {
    let env = Env::new();
    env.init();
    // Warnings travel in stdout (`warnings`), so a captured stderr stays empty on success.
    let added = env
        .cmd()
        .args([
            "--json",
            "job",
            "add",
            "https://www.linkedin.com/jobs/view/3812345678",
            "--title",
            "T",
            "--company",
            "C",
        ])
        .output()
        .unwrap();
    assert!(added.status.success());
    assert!(added.stderr.is_empty(), "{}", String::from_utf8_lossy(&added.stderr));
    let v: serde_json::Value = serde_json::from_slice(&added.stdout).unwrap();
    assert!(v["warnings"][0].as_str().unwrap().contains("tracked only"));

    // --quiet is accepted everywhere and never hides an error.
    let quiet = env.cmd().args(["--quiet", "job", "list"]).output().unwrap();
    assert!(quiet.status.success() && quiet.stderr.is_empty());
    let failing = env.cmd().args(["--quiet", "job", "show", "oa_00000000"]).output().unwrap();
    assert_eq!(failing.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&failing.stderr).starts_with("error:"));
}
