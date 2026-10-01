//! Keeps the written material honest: the README, skill, docs and workflows must agree with the binary.

mod common;

use std::path::PathBuf;

use common::Env;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

const COMMANDS: &[&str] = &[
    "init",
    "doctor",
    "profile show",
    "profile path",
    "profile set",
    "source add",
    "source list",
    "source remove",
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
];

#[test]
fn readme_documents_every_command_and_exit_code() {
    let readme = read("README.md");
    for cmd in COMMANDS {
        assert!(readme.contains(&format!("open-apply {cmd}")), "README does not document `open-apply {cmd}`");
    }
    // The exit code table must match what the binary reports.
    let env = Env::new();
    let v = env.ok(&["agent-readme"]);
    for c in v["exit_codes"].as_array().unwrap() {
        let row = format!("| `{}` | `{}` |", c["code"], c["name"].as_str().unwrap());
        assert!(readme.contains(&row), "README exit code table is missing {row}");
    }
    assert!(readme.contains("cargo install --git https://github.com/SpaceCorps/open-apply --locked"));
    assert!(readme.contains("github.com/SpaceCorps/open-apply/actions/workflows/ci.yml/badge.svg"));
    for section in [
        "## Highlights",
        "## Installation",
        "## Quickstart",
        "## Command Reference",
        "## Agent workflow loop",
        "## Design notes",
        "## Known limits and roadmap",
    ] {
        assert!(readme.contains(section), "README is missing {section}");
    }
}

#[test]
fn readme_states_the_safety_stance_plainly() {
    let readme = read("README.md");
    for claim in [
        "does not scrape LinkedIn",
        "does not bypass CAPTCHAs or bot detection",
        "does not store site passwords",
        "does not submit forms itself",
        "Low-quality mass-applying hurts the applicant",
    ] {
        assert!(readme.contains(claim), "README is missing: {claim}");
    }
}

#[test]
fn help_lists_what_the_readme_and_skill_tables_promise() {
    let env = Env::new();
    for cmd in COMMANDS {
        let mut args: Vec<&str> = cmd.split(' ').collect();
        args.push("--help");
        let out = env.cmd().args(&args).output().unwrap();
        assert!(out.status.success(), "`open-apply {cmd} --help` failed");
    }
}

#[test]
fn copy_has_no_emojis_or_em_dashes() {
    for file in ["README.md", "AGENTS.md", "skills/open-apply/SKILL.md", "docs/agent-guide.md", "docs/schema.md"] {
        let text = read(file);
        for (n, line) in text.lines().enumerate() {
            assert!(
                line.is_ascii(),
                "{file}:{}: non-ASCII character (no emojis or em-dashes in the copy): {line}",
                n + 1
            );
        }
    }
}

#[test]
fn metadata_matches_the_spdx_and_toolchain_conventions() {
    let cargo = read("Cargo.toml");
    assert!(cargo.contains("edition = \"2024\""));
    assert!(cargo.contains("rust-version = \"1.89\""));
    assert!(cargo.contains("license = \"MIT\""));
    assert!(cargo.contains("repository = \"https://github.com/SpaceCorps/open-apply\""));
    for profile in ["lto = \"fat\"", "codegen-units = 1", "panic = \"abort\"", "strip = true"] {
        assert!(cargo.contains(profile), "release profile is missing {profile}");
    }
    assert_eq!(read("rustfmt.toml").trim(), "max_width = 120\nuse_small_heuristics = \"Max\"");
    let license = read("LICENSE");
    assert!(license.starts_with("MIT License"));
    assert!(license.contains("Copyright (c) 2026 Mikael Rinne"));
    assert!(license.contains("Copyright (c) 2026 SpaceCorps"));
}

fn workflow(name: &str) -> serde_norway::Value {
    serde_norway::from_str(&read(&format!(".github/workflows/{name}")))
        .unwrap_or_else(|e| panic!("{name} is not valid YAML: {e}"))
}

#[test]
fn ci_workflow_runs_fmt_clippy_and_tests_on_three_platforms() {
    let text = read(".github/workflows/ci.yml");
    let wf = workflow("ci.yml");
    assert!(wf["jobs"]["test"].is_mapping());
    for os in ["ubuntu-latest", "macos-latest", "windows-latest"] {
        assert!(text.contains(os), "ci.yml does not run on {os}");
    }
    for step in ["cargo fmt --check", "cargo clippy --all-targets --locked -- -D warnings", "cargo test --locked"] {
        assert!(text.contains(step), "ci.yml is missing `{step}`");
    }
}

#[test]
fn release_workflow_builds_every_target_from_a_tag() {
    let text = read(".github/workflows/release.yml");
    let wf = workflow("release.yml");
    assert!(wf["jobs"]["build"].is_mapping() && wf["jobs"]["publish"].is_mapping());
    assert!(text.contains("tags: [ \"v*\" ]"));
    for target in [
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "x86_64-unknown-linux-musl",
        "aarch64-unknown-linux-musl",
        "x86_64-pc-windows-msvc",
    ] {
        assert!(text.contains(target), "release.yml does not build {target}");
    }
    assert!(text.contains("open-apply-${GITHUB_REF_NAME}-"), "archives are named open-apply-<tag>-<target>");
}

#[test]
fn pages_workflow_builds_the_site_with_vite_plus() {
    let text = read(".github/workflows/pages.yml");
    let wf = workflow("pages.yml");
    assert!(wf["jobs"]["build"].is_mapping() && wf["jobs"]["deploy"].is_mapping());
    for step in
        ["voidzero-dev/setup-vp", "vp install", "vp check", "vp build", "actions/deploy-pages", "path: site/dist"]
    {
        assert!(text.contains(step), "pages.yml is missing `{step}`");
    }
}

#[test]
fn docs_folder_has_the_guides() {
    for file in ["docs/agent-guide.md", "docs/schema.md", "skills/open-apply/SKILL.md", "AGENTS.md"] {
        assert!(root().join(file).is_file(), "{file} is missing");
    }
    // The schema doc names every table and the trigger that keeps events append-only.
    let schema = read("docs/schema.md");
    for needle in ["`jobs`", "`events`", "`sources`", "events_append_only", "user_version"] {
        assert!(schema.contains(needle), "docs/schema.md is missing {needle}");
    }
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The site's command reference is generated from `--help` (site/scripts/gen-commands.mjs). If this
/// fails, rebuild the binary and run `node site/scripts/gen-commands.mjs`.
#[test]
fn site_command_reference_matches_the_cli_help() {
    let data: serde_json::Value = serde_json::from_str(&read("site/src/generated/commands.json")).unwrap();
    assert_eq!(data["version"], env!("CARGO_PKG_VERSION"));
    let env = Env::new();
    let commands = data["commands"].as_array().unwrap();
    assert_eq!(commands.len(), COMMANDS.len());

    for c in commands {
        let path = c["path"].as_str().unwrap();
        assert!(COMMANDS.contains(&path), "the site documents `{path}`, which is not a command");
        let mut args: Vec<&str> = path.split(' ').collect();
        args.push("--help");
        let out = env.cmd().args(&args).output().unwrap();
        let help = squash(&String::from_utf8(out.stdout).unwrap());
        assert!(
            help.contains(&squash(c["about"].as_str().unwrap())),
            "`{path}`: the about text changed, regenerate the site data"
        );
        assert!(
            help.contains(&squash(c["usage"].as_str().unwrap())),
            "`{path}`: usage changed, regenerate the site data"
        );
        for o in c["options"].as_array().unwrap() {
            assert!(help.contains(&squash(o["flag"].as_str().unwrap())), "`{path}`: option {} is gone", o["flag"]);
            assert!(
                help.contains(&squash(o["description"].as_str().unwrap())),
                "`{path}`: option text changed for {}",
                o["flag"]
            );
        }
    }
    // And the other way: nothing in the top-level help is missing from the site.
    let top = String::from_utf8(env.cmd().arg("--help").output().unwrap().stdout).unwrap();
    let groups: std::collections::BTreeSet<&str> = commands.iter().map(|c| c["group"].as_str().unwrap()).collect();
    let listed: Vec<&str> = top
        .lines()
        .skip_while(|l| !l.starts_with("Commands:"))
        .skip(1)
        .take_while(|l| !l.trim().is_empty())
        .map(|l| l.split_whitespace().next().unwrap())
        .filter(|name| *name != "help")
        .collect();
    assert_eq!(listed.len(), groups.len());
    for name in listed {
        assert!(groups.contains(name), "`{name}` is in --help but not in the site reference");
    }
}
