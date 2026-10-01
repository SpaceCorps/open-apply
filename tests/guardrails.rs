//! Guardrails: daily cap, company cooldown, required materials, and `--force`.

mod common;

use common::Env;

fn events(env: &Env, id: &str) -> Vec<serde_json::Value> {
    env.ok(&["job", "show", id])["events"].as_array().unwrap().clone()
}

#[test]
fn daily_cap_blocks_applied_and_next_until_forced() {
    let env = Env::new();
    env.init();
    env.write_config("daily_application_cap: 2\n");
    let ids: Vec<String> = (1..=4)
        .map(|i| env.add_job(&format!("https://c{i}.example/jobs/{i}"), &format!("Company{i}"), "Engineer"))
        .collect();

    assert_eq!(env.ok(&["applied", &ids[0]])["remaining_today"], 1);
    assert_eq!(env.ok(&["applied", &ids[1]])["remaining_today"], 0);

    let err = env.fails(6, &["applied", &ids[2]]);
    assert!(
        err["error"]["message"]
            .as_str()
            .unwrap()
            .contains("daily cap reached: 2 application(s) in the last 24 hours, cap is 2")
    );
    assert!(err["error"]["hint"].as_str().unwrap().contains("--force"));
    assert_eq!(env.ok(&["job", "show", &ids[2]])["status"], "saved", "a blocked application changes nothing");

    // `next` refuses to hand out more work today, with the numbers.
    let err = env.fails(6, &["next"]);
    assert_eq!(err["error"]["detail"]["guardrails"]["applied_last_24h"], 2);
    assert_eq!(err["error"]["detail"]["guardrails"]["remaining_today"], 0);
    let forced_next = env.ok(&["next", "--force"]);
    assert_eq!(forced_next["count"], 2);

    // --force goes through, says so, and leaves a trace in the history.
    let forced = env.ok(&["applied", &ids[2], "--force", "--note", "strong referral"]);
    assert_eq!(forced["forced"], true);
    assert_eq!(forced["overridden"][0]["guardrail"], "daily_cap");
    let evs = events(&env, &ids[2]);
    let applied = evs.iter().find(|e| e["type"] == "applied").unwrap();
    assert_eq!(applied["note"], "strong referral | forced past: daily_cap");

    // An application dated outside the 24 hour window does not count against today.
    let old = env.ok(&["applied", &ids[3], "--at", "2026-09-25"]);
    assert_eq!(old["applied_last_24h"], 1);
    assert!(old.get("forced").is_none());
}

#[test]
fn next_is_shortened_to_what_is_left_of_the_cap() {
    let env = Env::new();
    env.init();
    env.write_config("daily_application_cap: 2\n");
    let ids: Vec<String> = (1..=4)
        .map(|i| env.add_job(&format!("https://c{i}.example/jobs/{i}"), &format!("Company{i}"), "Engineer"))
        .collect();
    env.ok(&["applied", &ids[0]]);
    let next = env.ok(&["next", "--count", "5"]);
    assert_eq!(next["count"], 1);
    assert_eq!(next["guardrails"]["remaining_today"], 1);
    assert!(next["hint"].as_str().unwrap().contains("cap"));
    assert_eq!(env.ok(&["next", "--count", "5", "--force"])["count"], 3);
}

#[test]
fn zero_cap_means_no_cap() {
    let env = Env::new();
    env.init();
    env.write_config("daily_application_cap: 0\ncompany_cooldown_days: 0\n");
    for i in 1..=3 {
        let id = env.add_job(&format!("https://same.example/jobs/{i}"), "Same Co", &format!("Role {i}"));
        env.ok(&["applied", &id]);
    }
    assert!(env.ok(&["next"])["guardrails"]["remaining_today"].is_null());
}

#[test]
fn company_cooldown_blocks_a_second_role_at_the_same_company() {
    let env = Env::new();
    env.init(); // default cooldown: 90 days
    let a = env.add_job("https://globex.example/jobs/1", "Globex Inc.", "Backend Engineer");
    let b = env.add_job("https://globex.example/jobs/2", "GLOBEX", "Frontend Engineer");
    let other = env.add_job("https://initech.example/jobs/1", "Initech", "Engineer");

    env.ok(&["applied", &a, "--at", "2026-08-01"]); // 61 days before the pinned clock
    let err = env.fails(6, &["applied", &b]);
    let v = &err["error"]["detail"]["violations"][0];
    assert_eq!(v["guardrail"], "company_cooldown");
    assert!(v["message"].as_str().unwrap().contains("Backend Engineer"));
    env.ok(&["applied", &other]); // other companies are unaffected

    // `next` leaves the blocked role out, says why, and still offers everything else.
    let fresh = env.add_job("https://umbrella.example/jobs/1", "Umbrella", "Engineer");
    let next = env.ok(&["next"]);
    assert_eq!(next["count"], 1);
    assert_eq!(next["jobs"][0]["id"], fresh.as_str());
    assert_eq!(next["blocked"][0]["id"], b.as_str());
    assert_eq!(next["blocked"][0]["reasons"][0]["guardrail"], "company_cooldown");
}

#[test]
fn cooldown_passes_once_enough_days_have_gone_by() {
    let env = Env::new();
    env.init();
    let a = env.add_job("https://globex.example/jobs/1", "Globex", "Backend Engineer");
    let b = env.add_job("https://globex.example/jobs/2", "Globex", "Frontend Engineer");
    env.ok(&["applied", &a, "--at", "2026-05-01"]); // 153 days before the pinned clock
    env.ok(&["applied", &b]);
}

#[test]
fn next_fails_when_every_candidate_is_blocked() {
    let env = Env::new();
    env.init();
    let a = env.add_job("https://globex.example/jobs/1", "Globex", "Backend Engineer");
    env.add_job("https://globex.example/jobs/2", "Globex", "Frontend Engineer");
    env.ok(&["applied", &a, "--at", "2026-09-20"]);
    let err = env.fails(6, &["next"]);
    assert!(err["error"]["message"].as_str().unwrap().contains("blocked by guardrails"));
    assert_eq!(err["error"]["detail"]["blocked"].as_array().unwrap().len(), 1);
    let forced = env.ok(&["next", "--force"]);
    assert_eq!(forced["count"], 1);
    assert_eq!(forced["jobs"][0]["forced_past"][0]["guardrail"], "company_cooldown");
}

#[test]
fn require_materials_needs_a_cv_that_exists() {
    let env = Env::new();
    env.init();
    env.write_config("require_materials: true\n");
    let id = env.add_job("https://a.example/jobs/1", "Acme", "Engineer");

    let err = env.fails(6, &["applied", &id]);
    assert_eq!(err["error"]["detail"]["violations"][0]["guardrail"], "missing_materials");
    let err = env.fails(6, &["next"]);
    assert_eq!(err["error"]["detail"]["blocked"][0]["reasons"][0]["guardrail"], "missing_materials");

    // A profile CV path that does not exist is not materials.
    env.ok(&["profile", "set", "cv_path", env.dir.path().join("missing.pdf").to_str().unwrap()]);
    env.fails(6, &["applied", &id]);

    let cv = env.dir.path().join("cv.pdf");
    std::fs::write(&cv, "%PDF").unwrap();
    env.ok(&["materials", "attach", &id, "--cv", cv.to_str().unwrap()]);
    assert_eq!(env.ok(&["next"])["count"], 1);
    env.ok(&["applied", &id]);
}

#[test]
fn profile_cv_counts_as_materials() {
    let env = Env::new();
    env.init();
    env.write_config("require_materials: true\n");
    let cv = env.dir.path().join("cv.pdf");
    std::fs::write(&cv, "%PDF").unwrap();
    env.ok(&["profile", "set", "cv_path", cv.to_str().unwrap()]);
    let id = env.add_job("https://a.example/jobs/1", "Acme", "Engineer");
    env.ok(&["applied", &id]);
}

#[test]
fn applied_rules_outside_the_guardrails() {
    let env = Env::new();
    env.init();
    let id = env.add_job("https://a.example/jobs/1", "Acme", "Engineer");
    // Channel is inferred when not given, and validated when given.
    let v = env.ok(&["applied", &id, "--via", "referral"]);
    assert_eq!(v["via"], "referral");
    env.fails(7, &["applied", &id]);
    // Closed roles cannot be applied to.
    let closed = env.add_job("https://b.example/jobs/1", "Globex", "Engineer");
    env.ok(&["status", &closed, "closed"]);
    env.fails(4, &["applied", &closed]);
    // 'applied' cannot be set through `status`, which would skip the guardrails.
    let third = env.add_job("https://c.example/jobs/1", "Initech", "Engineer");
    env.fails(4, &["status", &third, "applied"]);
}
