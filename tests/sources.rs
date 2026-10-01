//! Search and `job add` against recorded fixtures served by an in-process mock. No live network.

mod common;

use common::{Env, Mock};
use serde_json::Value;

fn titles(v: &Value) -> Vec<String> {
    v["results"].as_array().unwrap().iter().map(|r| r["title"].as_str().unwrap().to_string()).collect()
}

#[test]
fn search_reads_every_supported_feed() {
    let mock = Mock::feeds();
    let env = Env::with_mock(&mock);
    env.init();
    let v = env.ok(&[
        "search",
        "--source",
        "greenhouse:acme-labs",
        "--source",
        "lever:acme",
        "--source",
        "ashby:acme",
        "--source",
        "remoteok",
        "--source",
        "weworkremotely",
        "--source",
        "arbeitnow",
        "--limit",
        "50",
    ]);
    assert_eq!(v["matched"], 12);
    assert_eq!(v["count"], 12);
    let per_source = v["sources"].as_array().unwrap();
    assert_eq!(per_source.len(), 6);
    assert!(per_source.iter().all(|s| s["fetched"] == 2 && s["matched"] == 2), "{per_source:?}");
    assert!(v.get("errors").is_none());

    // Every request identified itself honestly.
    let reqs = mock.requests();
    assert_eq!(reqs.len(), 6);
    for r in &reqs {
        assert!(r.user_agent.starts_with("open-apply/"), "{}", r.user_agent);
        assert!(r.user_agent.contains("(+https://github.com/SpaceCorps/open-apply)"), "{}", r.user_agent);
    }
    assert!(reqs.iter().any(|r| r.path == "/v1/boards/acme-labs/jobs" && r.query == "content=true"));
    assert!(reqs.iter().any(|r| r.path == "/v0/postings/acme" && r.query == "mode=json"));

    // Without --save nothing is stored.
    assert_eq!(env.ok(&["job", "list"])["count"], 0);
}

#[test]
fn search_filters_and_limit() {
    let mock = Mock::feeds();
    let env = Env::with_mock(&mock);
    env.init();
    let all = [
        "--source",
        "greenhouse:acme-labs",
        "--source",
        "lever:acme",
        "--source",
        "ashby:acme",
        "--source",
        "remoteok",
        "--source",
        "weworkremotely",
        "--source",
        "arbeitnow",
    ];

    let mut args = vec!["search"];
    args.extend(all);
    args.extend(["--query", "rust", "--remote"]);
    let v = env.ok(&args);
    let mut got = titles(&v);
    got.sort();
    assert_eq!(got, ["Platform Engineer", "Senior Rust Engineer", "Senior Rust Engineer", "Senior Rust Engineer"]);

    let mut args = vec!["search"];
    args.extend(all);
    args.extend(["--location", "berlin"]);
    let got = titles(&env.ok(&args));
    assert!(got.contains(&"Rust Developer".to_string()) && got.contains(&"Product Designer".to_string()), "{got:?}");
    assert_eq!(got.len(), 4); // greenhouse designer, ashby x2 (Berlin, Germany), arbeitnow developer

    // --limit samples across feeds instead of taking the first feed whole.
    let mut args = vec!["search"];
    args.extend(all);
    args.extend(["--limit", "6"]);
    let v = env.ok(&args);
    assert_eq!(v["count"], 6);
    assert_eq!(v["matched"], 12);
    let sources: std::collections::BTreeSet<&str> =
        v["results"].as_array().unwrap().iter().map(|r| r["source"].as_str().unwrap()).collect();
    assert_eq!(sources.len(), 6);
}

#[test]
fn search_save_stores_leads_and_dedupes_on_rerun() {
    let mock = Mock::feeds();
    let env = Env::with_mock(&mock);
    env.init();
    env.ok(&["source", "add", "greenhouse:acme-labs"]);
    env.ok(&["source", "add", "remoteok"]);
    let listed = env.ok(&["source", "list"]);
    assert_eq!(listed["count"], 2);
    assert_eq!(env.ok(&["source", "add", "remoteok"])["added"], false);

    // No --source: the watched feeds are used.
    let first = env.ok(&["search", "--query", "rust", "--save"]);
    assert_eq!(first["saved"], 2);
    assert_eq!(first["duplicates"], 0);
    let ids: Vec<&str> = first["results"].as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(ids.len(), 2);

    let second = env.ok(&["search", "--query", "rust", "--save"]);
    assert_eq!(second["saved"], 0);
    assert_eq!(second["duplicates"], 2);
    assert_eq!(second["results"][0]["duplicate_of"], first["results"][0]["id"]);

    let leads = env.ok(&["job", "list", "--status", "lead"]);
    assert_eq!(leads["count"], 2);
    let shown = env.ok(&["job", "show", ids[0]]);
    assert_eq!(shown["status"], "lead");
    assert!(shown["description"].as_str().unwrap().contains("untrusted job content"));
    // Leads land in the work queue.
    assert_eq!(env.ok(&["next"])["count"], 2);

    assert_eq!(env.ok(&["source", "remove", "remoteok"])["removed"], true);
    assert_eq!(env.ok(&["source", "list"])["count"], 1);
}

#[test]
fn one_failing_feed_does_not_sink_the_search() {
    let mock = Mock::feeds();
    let env = Env::with_mock(&mock);
    env.init();
    let out = env
        .cmd()
        .env("OPEN_APPLY_LEVER_URL", "http://127.0.0.1:9")
        .args(["--json", "search", "--source", "lever:acme", "--source", "arbeitnow"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["count"], 2);
    let errors = v["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0]["source"], "lever:acme");
    assert_eq!(errors[0]["code"], "network");
}

#[test]
fn all_feeds_failing_is_a_network_error_with_details() {
    let env = Env::new();
    env.init();
    let err = env.fails(5, &["search", "--source", "lever:acme", "--source", "arbeitnow"]);
    assert_eq!(err["error"]["code"], "network");
    assert_eq!(err["error"]["detail"].as_array().unwrap().len(), 2);
}

#[test]
fn job_add_reads_greenhouse_lever_and_ashby_urls() {
    let mock = Mock::feeds();
    let env = Env::with_mock(&mock);
    env.init();

    let gh = env.ok(&["job", "add", "https://job-boards.greenhouse.io/acme-labs/jobs/4012345"]);
    assert_eq!(gh["job"]["title"], "Senior Rust Engineer");
    assert_eq!(gh["job"]["location"], "Remote - Europe");
    assert!(gh["description_chars"].as_u64().unwrap() > 50);

    let lever = env.ok(&[
        "job",
        "add",
        "https://jobs.lever.co/acme/0f1e2d3c-aaaa-bbbb-cccc-1234567890ab/apply?lever-source=LinkedIn",
    ]);
    assert_eq!(lever["job"]["title"], "Platform Engineer");
    assert_eq!(lever["job"]["company"], "Acme");
    assert_eq!(lever["job"]["source"], "lever:acme");

    let ashby = env.ok(&["job", "add", "https://jobs.ashbyhq.com/acme/11111111-2222-3333-4444-555555555555"]);
    assert_eq!(ashby["job"]["title"], "Staff Backend Engineer");
    assert_eq!(ashby["job"]["source"], "ashby:acme");

    // Explicit flags win over what the endpoint says.
    let over = env.ok(&[
        "job",
        "add",
        "https://jobs.lever.co/acme/9a8b7c6d-0000-1111-2222-abcdefabcdef",
        "--no-fetch",
        "--title",
        "Mine",
        "--company",
        "Acme",
    ]);
    assert_eq!(over["job"]["title"], "Mine");
    assert_eq!(over["fetched"], false);
}

#[test]
fn job_add_closed_posting_is_not_found() {
    let mock = Mock::feeds();
    let env = Env::with_mock(&mock);
    env.init();
    let err = env.fails(3, &["job", "add", "https://boards.greenhouse.io/acme-labs/jobs/999"]);
    assert_eq!(err["error"]["code"], "not_found");
    // Known title and company: store it anyway, with a warning.
    let v = env.ok(&[
        "job",
        "add",
        "https://boards.greenhouse.io/acme-labs/jobs/999",
        "--title",
        "Old role",
        "--company",
        "Acme Labs",
    ]);
    assert_eq!(v["job"]["title"], "Old role");
    assert!(v["warnings"][0].as_str().unwrap().contains("could not read the page"));
}

#[test]
fn job_add_parses_json_ld_on_other_pages() {
    let mock = Mock::feeds();
    let env = Env::with_mock(&mock);
    env.init();
    let url = format!("{}/careers/42", mock.url);
    let v = env.ok(&["job", "add", &url]);
    assert_eq!(v["job"]["title"], "Data Engineer");
    assert_eq!(v["job"]["company"], "Example Oy");
    assert_eq!(v["job"]["location"], "Helsinki, Uusimaa, FI");
    assert_eq!(v["job"]["source"], "manual");
    assert_eq!(v["fetched"], true);
    let shown = env.ok(&["job", "show", v["job"]["id"].as_str().unwrap()]);
    assert!(shown["description"].as_str().unwrap().contains("Own the data platform."));
}

#[test]
fn job_add_degrades_to_what_was_given_when_the_page_has_nothing() {
    let mock = Mock::feeds();
    let env = Env::with_mock(&mock);
    env.init();
    let plain = format!("{}/careers/plain", mock.url);
    let v = env.ok(&[
        "job",
        "add",
        &plain,
        "--title",
        "Hand entered",
        "--company",
        "Nowhere Ltd",
        "--notes",
        "from a friend",
    ]);
    assert_eq!(v["job"]["title"], "Hand entered");
    assert!(v["warnings"][0].as_str().unwrap().contains("JobPosting"));
    assert_eq!(env.ok(&["job", "show", v["job"]["id"].as_str().unwrap()])["notes"], "from a friend");

    // A page that refuses automated access is reported, never worked around.
    let blocked = format!("{}/careers/blocked", mock.url);
    let v = env.ok(&["job", "add", &blocked, "--title", "Behind a wall", "--company", "Wall Inc"]);
    assert!(v["warnings"][0].as_str().unwrap().contains("refused"));
    // Without a title there is nothing to store.
    env.fails(4, &["job", "add", &blocked]);
    let hits = mock.requests().iter().filter(|r| r.path == "/careers/blocked").count();
    assert_eq!(hits, 2, "one fetch per attempt and no retries");
}

#[test]
fn tracking_only_sites_are_never_fetched() {
    let mock = Mock::feeds();
    let env = Env::with_mock(&mock);
    env.init();
    let v = env.ok(&[
        "job",
        "add",
        "https://www.linkedin.com/jobs/view/senior-rust-engineer-at-acme-3812345678/?trackingId=abc&refId=def",
        "--title",
        "Senior Rust Engineer",
        "--company",
        "Acme",
    ]);
    assert_eq!(v["job"]["tracking_only"], true);
    assert_eq!(v["fetched"], false);
    assert!(v["warnings"][0].as_str().unwrap().contains("never fetches or scrapes"));
    let indeed = env.ok(&[
        "job",
        "add",
        "https://fi.indeed.com/viewjob?jk=abc123&from=serp",
        "--title",
        "T",
        "--company",
        "Globex",
    ]);
    assert_eq!(indeed["job"]["tracking_only"], true);
    assert!(mock.requests().is_empty(), "no request may be made for tracking-only sites");
    // Without a title there is nothing to go on, and still no request.
    env.fails(4, &["job", "add", "https://www.linkedin.com/jobs/view/3999999999"]);
    assert!(mock.requests().is_empty());

    // Different LinkedIn URL shapes for the same posting are one job.
    let err = env
        .fails(7, &["job", "add", "https://www.linkedin.com/jobs/view/3812345678", "--title", "x", "--company", "y"]);
    assert_eq!(err["error"]["detail"]["matched_by"], "url");
    let err = env.fails(
        7,
        &[
            "job",
            "add",
            "https://fi.linkedin.com/jobs/search/?currentJobId=3812345678&keywords=rust",
            "--title",
            "x",
            "--company",
            "y",
        ],
    );
    assert_eq!(err["error"]["code"], "conflict");
}

#[test]
fn same_listing_from_another_url_is_a_duplicate_unless_allowed() {
    let env = Env::new();
    env.init();
    let add = |url: &str| {
        env.cmd()
            .args([
                "--json",
                "job",
                "add",
                url,
                "--no-fetch",
                "--title",
                "Rust Engineer",
                "--company",
                "Acme Inc.",
                "--location",
                "Berlin",
            ])
            .output()
            .unwrap()
    };
    assert!(add("https://acme.example/careers/1").status.success());
    let dup = add("https://board.example/jobs/99");
    assert_eq!(dup.status.code(), Some(7));
    let err: Value = serde_json::from_slice(&dup.stderr).unwrap();
    assert_eq!(err["error"]["detail"]["matched_by"], "listing");
    let allowed = env
        .cmd()
        .args([
            "--json",
            "job",
            "add",
            "https://board.example/jobs/99",
            "--no-fetch",
            "--allow-duplicate",
            "--title",
            "Rust Engineer",
            "--company",
            "Acme",
            "--location",
            "Berlin",
        ])
        .output()
        .unwrap();
    assert!(allowed.status.success());
    assert_eq!(env.ok(&["job", "list"])["count"], 2);
}
