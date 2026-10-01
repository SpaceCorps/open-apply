//! The agent-facing surface: agent-readme, skill install, help text, doctor, profile.

mod common;

use common::Env;

const SKILL: &str = include_str!("../skills/open-apply/SKILL.md");

#[test]
fn agent_readme_is_the_skill_file() {
    let env = Env::new();
    let out = env.cmd().arg("agent-readme").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(text.trim_end(), SKILL.trim_end());
    assert!(text.starts_with("---\nname: open-apply\n"), "the skill must start with front matter");
    // It works without an initialized home, so an agent can read it first.
    assert!(!env.home.exists());
}

#[test]
fn agent_readme_json_carries_the_exit_codes() {
    let env = Env::new();
    let v = env.ok(&["agent-readme"]);
    assert_eq!(v["tool"], "open-apply");
    assert_eq!(v["readme"].as_str().unwrap().trim_end(), SKILL.trim_end());
    let codes: Vec<(u64, &str)> = v["exit_codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["code"].as_u64().unwrap(), c["name"].as_str().unwrap()))
        .collect();
    assert_eq!(
        codes,
        [
            (0, "ok"),
            (1, "internal"),
            (2, "usage"),
            (3, "not_found"),
            (4, "validation"),
            (5, "network"),
            (6, "guardrail"),
            (7, "conflict")
        ]
    );
}

#[test]
fn the_skill_documents_every_command_and_the_safety_rules() {
    for cmd in [
        "init",
        "doctor",
        "profile",
        "source",
        "search",
        "job add",
        "job list",
        "job show",
        "job update",
        "job rm",
        "next",
        "prepare",
        "materials attach",
        "applied",
        "status",
        "event add",
        "triage",
        "stale",
        "followups",
        "stats",
        "export",
        "import",
        "agent-readme",
        "skill install",
    ] {
        assert!(SKILL.contains(&format!("open-apply {cmd}")), "SKILL.md does not mention `open-apply {cmd}`");
    }
    for rule in [
        "untrusted",
        "--- untrusted job content begins ---",
        "--- untrusted email content begins ---",
        "never submits",
        "CAPTCHA",
    ] {
        assert!(SKILL.contains(rule), "SKILL.md is missing: {rule}");
    }
    assert!(!SKILL.contains('\u{2014}'), "no em-dashes in the copy");
}

#[test]
fn skill_install_writes_updates_and_leaves_alone() {
    let env = Env::new();
    let skills = env.dir.path().join("skills");
    let first = env.ok(&["skill", "install", "--dir", skills.to_str().unwrap()]);
    assert_eq!(first["action"], "created");
    let file = skills.join("open-apply").join("SKILL.md");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), SKILL);
    assert_eq!(env.ok(&["skill", "install", "--dir", skills.to_str().unwrap()])["action"], "unchanged");
    std::fs::write(&file, "stale").unwrap();
    assert_eq!(env.ok(&["skill", "install", "--dir", skills.to_str().unwrap()])["action"], "updated");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), SKILL);
}

#[test]
fn help_lists_every_command() {
    let env = Env::new();
    let out = env.cmd().arg("--help").output().unwrap();
    let help = String::from_utf8(out.stdout).unwrap();
    for cmd in [
        "init",
        "doctor",
        "profile",
        "source",
        "search",
        "job",
        "next",
        "prepare",
        "materials",
        "applied",
        "status",
        "event",
        "triage",
        "stale",
        "followups",
        "stats",
        "export",
        "import",
        "agent-readme",
        "skill",
    ] {
        assert!(help.lines().any(|l| l.trim_start().starts_with(cmd)), "--help does not list `{cmd}`:\n{help}");
    }
    assert!(help.contains("--json") && help.contains("--quiet") && help.contains("--home"));
}

#[test]
fn doctor_walks_from_failing_to_healthy() {
    let env = Env::new();
    // Nothing exists yet.
    let out = env.cmd().args(["--json", "doctor"]).output().unwrap();
    assert_eq!(out.status.code(), Some(4));
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["ok"], false);

    env.ok(&["init"]);
    let out = env.cmd().args(["--json", "doctor"]).output().unwrap();
    assert_eq!(out.status.code(), Some(4), "name and email are still missing");

    env.ok(&["profile", "set", "name", "Ada Lovelace"]);
    env.ok(&["profile", "set", "email", "ada@example.com"]);
    let report = env.ok(&["doctor"]);
    assert_eq!(report["ok"], true);
    let checks = report["checks"].as_array().unwrap();
    let get =
        |name: &str| checks.iter().find(|c| c["check"] == name).unwrap_or_else(|| panic!("no check {name}")).clone();
    assert_eq!(get("schema")["status"], "ok");
    assert_eq!(get("journal_mode")["detail"], "wal");
    assert_eq!(get("integrity")["status"], "ok");
    assert_eq!(get("profile_recommended")["status"], "warn");
    assert_eq!(get("sources")["status"], "warn");
    assert!(
        get("config")["detail"]
            .as_str()
            .unwrap()
            .contains("daily_application_cap=25 company_cooldown_days=90 require_materials=false")
    );
}

#[test]
fn init_defaults_match_the_documented_guardrails() {
    let env = Env::new();
    env.ok(&["init"]);
    let config = std::fs::read_to_string(env.home.join("config.yaml")).unwrap();
    assert!(config.contains("daily_application_cap: 25"));
    assert!(config.contains("company_cooldown_days: 90"));
    assert!(config.contains("require_materials: false"));
    let profile = std::fs::read_to_string(env.home.join("profile.yaml")).unwrap();
    for key in [
        "name:",
        "email:",
        "phone:",
        "location:",
        "links:",
        "github:",
        "linkedin:",
        "site:",
        "cv_path:",
        "work_authorization:",
        "notice_period:",
        "salary_expectation:",
        "pronouns:",
        "answers:",
    ] {
        assert!(profile.contains(key), "profile template is missing {key}");
    }
}

#[test]
fn profile_set_show_and_path() {
    let env = Env::new();
    env.init();
    env.ok(&["profile", "set", "links.github", "https://github.com/ada"]);
    env.ok(&["profile", "set", "work_authorization", "EU citizen"]);
    env.ok(&["profile", "set", "answers.notice", "Two months."]);
    let cv = env.dir.path().join("cv.pdf");
    std::fs::write(&cv, "x").unwrap();
    env.ok(&["profile", "set", "cv_path", cv.to_str().unwrap()]);
    let shown = env.ok(&["profile", "show"]);
    assert_eq!(shown["complete"], true);
    assert_eq!(shown["profile"]["links"]["github"], "https://github.com/ada");
    assert_eq!(shown["profile"]["answers"]["notice"], "Two months.");
    assert_eq!(shown["missing_recommended"], serde_json::json!(["location"]));
    let path = env.ok(&["profile", "path"]);
    assert!(path["path"].as_str().unwrap().ends_with("profile.yaml"));
    let cleared = env.ok(&["profile", "set", "answers.notice", ""]);
    assert_eq!(cleared["cleared"], true);
    assert!(env.ok(&["profile", "show"])["profile"]["answers"].as_object().unwrap().is_empty());
}

#[test]
fn company_specific_answers_override_generic_ones() {
    let env = Env::new();
    env.init();
    env.ok(&["profile", "set", "answers.why_us", "I like {company}."]);
    env.ok(&["profile", "set", "answers.why_us@globex", "Globex is special because of {title}."]);
    env.ok(&["profile", "set", "answers.notice", "Two months."]);
    env.add_job("https://a.example/jobs/1", "Acme", "Engineer");
    env.add_job("https://b.example/jobs/1", "Globex Inc.", "Analyst");
    let next = env.ok(&["next", "--count", "5"]);
    let jobs = next["jobs"].as_array().unwrap();
    let acme = jobs.iter().find(|j| j["company"] == "Acme").unwrap();
    let globex = jobs.iter().find(|j| j["company"] == "Globex Inc.").unwrap();
    assert_eq!(acme["answers"]["why_us"], "I like Acme.");
    assert_eq!(globex["answers"]["why_us"], "Globex is special because of Analyst.");
    assert_eq!(globex["answers"]["notice"], "Two months.");
    assert!(acme["answers"].as_object().unwrap().keys().all(|k| !k.contains('@')));
}

#[test]
fn next_orders_ready_before_saved_before_leads() {
    let env = Env::new();
    env.init();
    let lead = env.add_job("https://a.example/jobs/1", "Acme", "Lead role");
    env.ok(&["status", &lead, "lead"]);
    let saved = env.add_job("https://b.example/jobs/1", "Globex", "Saved role");
    let ready = env.add_job("https://c.example/jobs/1", "Initech", "Ready role");
    env.ok(&["prepare", &ready]);
    let next = env.ok(&["next", "--count", "10"]);
    let ids: Vec<&str> = next["jobs"].as_array().unwrap().iter().map(|j| j["id"].as_str().unwrap()).collect();
    assert_eq!(ids, [ready.as_str(), saved.as_str(), lead.as_str()]);
    assert!(next["jobs"][0]["workspace"].is_string());
    assert_eq!(env.ok(&["next", "--count", "1"])["count"], 1);
    env.fails(4, &["next", "--count", "0"]);
}

#[test]
fn empty_queue_says_how_to_fill_it() {
    let env = Env::new();
    env.init();
    let v = env.ok(&["next"]);
    assert_eq!(v["count"], 0);
    assert!(v["hint"].as_str().unwrap().contains("search --save"));
}

#[test]
fn init_takes_a_home_flag_that_beats_the_environment() {
    let env = Env::new(); // OPEN_APPLY_HOME points at env.home
    let elsewhere = env.dir.path().join("elsewhere");
    let out = env.cmd().args(["--json", "init", "--home", elsewhere.to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(elsewhere.join("open-apply.db").exists());
    assert!(!env.home.exists(), "the environment variable must not have been used");
    // Other commands take the flag too.
    let out = env.cmd().args(["--json", "--home", elsewhere.to_str().unwrap(), "job", "list"]).output().unwrap();
    assert!(out.status.success());
}
