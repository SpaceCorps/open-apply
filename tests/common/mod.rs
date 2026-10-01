//! Shared helpers for the integration tests: an isolated home per test, a command builder that
//! can never reach the real network, and a tiny in-process HTTP server that serves fixtures.

#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

/// Pinned clock so stale/followup/cap math is deterministic.
pub const NOW: &str = "2026-10-01T12:00:00Z";

pub fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
}

pub struct Env {
    pub dir: TempDir,
    pub home: PathBuf,
    /// Base URL every source endpoint is redirected to. Unreachable unless a mock is attached.
    pub mock_url: String,
}

/// Where endpoints point when a test has no mock: a closed local port, so an accidental fetch
/// fails immediately instead of touching the internet.
const DEAD_URL: &str = "http://127.0.0.1:9";

impl Env {
    pub fn new() -> Env {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        Env { dir, home, mock_url: DEAD_URL.to_string() }
    }

    pub fn with_mock(mock: &Mock) -> Env {
        let mut env = Env::new();
        env.mock_url = mock.url.clone();
        env
    }

    pub fn cmd(&self) -> Command {
        let mut c = assert_cmd::cargo::cargo_bin_cmd!("open-apply");
        c.env("OPEN_APPLY_HOME", &self.home)
            .env("OPEN_APPLY_NOW", NOW)
            .env("OPEN_APPLY_REQUEST_DELAY_MS", "0")
            .env("OPEN_APPLY_GREENHOUSE_URL", &self.mock_url)
            .env("OPEN_APPLY_LEVER_URL", &self.mock_url)
            .env("OPEN_APPLY_ASHBY_URL", &self.mock_url)
            .env("OPEN_APPLY_REMOTEOK_URL", &self.mock_url)
            .env("OPEN_APPLY_WWR_URL", &self.mock_url)
            .env("OPEN_APPLY_ARBEITNOW_URL", &self.mock_url)
            .env_remove("HOME")
            .env_remove("USERPROFILE");
        c
    }

    /// Runs with `--json`, asserts exit 0 and returns stdout as JSON.
    pub fn ok(&self, args: &[&str]) -> Value {
        let out = self.cmd().arg("--json").args(args).output().unwrap();
        assert!(
            out.status.success(),
            "`{}` failed ({:?})\nstdout: {}\nstderr: {}",
            args.join(" "),
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {}", String::from_utf8_lossy(&out.stdout)))
    }

    /// Runs with `--json`, expects exit code `code`, and returns the parsed error envelope from stderr.
    pub fn fails(&self, code: i32, args: &[&str]) -> Value {
        let out = self.cmd().arg("--json").args(args).output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(code),
            "`{}` exit code\nstdout: {}\nstderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stderr)
            .unwrap_or_else(|e| panic!("stderr is not JSON ({e}): {}", String::from_utf8_lossy(&out.stderr)))
    }

    /// `init` plus a complete profile.
    pub fn init(&self) {
        self.ok(&["init"]);
        self.ok(&["profile", "set", "name", "Ada Lovelace"]);
        self.ok(&["profile", "set", "email", "ada@example.com"]);
    }

    pub fn write_config(&self, yaml: &str) {
        std::fs::write(self.home.join("config.yaml"), yaml).unwrap();
    }

    /// Adds a tracking-only job offline (no network involved) and returns its id.
    pub fn add_job(&self, url: &str, company: &str, title: &str) -> String {
        let v = self.ok(&["job", "add", url, "--company", company, "--title", title, "--no-fetch"]);
        v["job"]["id"].as_str().unwrap().to_string()
    }
}

#[derive(Clone)]
pub struct Request {
    pub path: String,
    pub query: String,
    pub user_agent: String,
}

pub struct Mock {
    pub url: String,
    pub log: Arc<Mutex<Vec<Request>>>,
}

impl Mock {
    /// `routes` maps an exact request path (no query) to `(status, content type, body)`.
    pub fn start(routes: Vec<(&str, u16, &str, String)>) -> Mock {
        let routes: HashMap<String, (u16, String, String)> =
            routes.into_iter().map(|(p, s, ct, b)| (p.to_string(), (s, ct.to_string(), b))).collect();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let log = Arc::new(Mutex::new(Vec::new()));
        let log2 = log.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let routes = routes.clone();
                let log = log2.clone();
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    let target = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
                    let mut user_agent = String::new();
                    let mut len = 0usize;
                    loop {
                        let mut h = String::new();
                        if reader.read_line(&mut h).unwrap_or(0) == 0 {
                            break;
                        }
                        let h = h.trim_end();
                        if h.is_empty() {
                            break;
                        }
                        if let Some((k, v)) = h.split_once(':') {
                            match k.trim().to_ascii_lowercase().as_str() {
                                "user-agent" => user_agent = v.trim().to_string(),
                                "content-length" => len = v.trim().parse().unwrap_or(0),
                                _ => {}
                            }
                        }
                    }
                    if len > 0 {
                        let mut sink = vec![0; len];
                        let _ = reader.read_exact(&mut sink);
                    }
                    log.lock().unwrap().push(Request { path: path.to_string(), query: query.to_string(), user_agent });
                    let (status, ctype, body) =
                        routes.get(path).cloned().unwrap_or((404, "text/plain".into(), "not found".into()));
                    let reason = match status {
                        200 => "OK",
                        404 => "Not Found",
                        403 => "Forbidden",
                        429 => "Too Many Requests",
                        _ => "Error",
                    };
                    let head = format!(
                        "HTTP/1.1 {status} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(body.as_bytes());
                });
            }
        });
        Mock { url, log }
    }

    pub fn requests(&self) -> Vec<Request> {
        self.log.lock().unwrap().clone()
    }

    /// A mock serving every feed fixture at the path its real endpoint uses.
    pub fn feeds() -> Mock {
        let single_gh = {
            let mut list: Value = serde_json::from_str(&fixture("greenhouse_jobs.json")).unwrap();
            let mut job = list["jobs"][0].take();
            job["company_name"] = Value::String("Acme Labs".into());
            job.to_string()
        };
        let single_lever = {
            let list: Value = serde_json::from_str(&fixture("lever_postings.json")).unwrap();
            list[0].to_string()
        };
        Mock::start(vec![
            ("/v1/boards/acme-labs/jobs", 200, "application/json", fixture("greenhouse_jobs.json")),
            ("/v1/boards/acme-labs/jobs/4012345", 200, "application/json", single_gh),
            ("/v0/postings/acme", 200, "application/json", fixture("lever_postings.json")),
            ("/v0/postings/acme/0f1e2d3c-aaaa-bbbb-cccc-1234567890ab", 200, "application/json", single_lever),
            ("/posting-api/job-board/acme", 200, "application/json", fixture("ashby_board.json")),
            ("/api", 200, "application/json", fixture("remoteok_api.json")),
            ("/remote-jobs.rss", 200, "application/rss+xml", fixture("weworkremotely.rss")),
            ("/api/job-board-api", 200, "application/json", fixture("arbeitnow_api.json")),
            ("/careers/42", 200, "text/html", fixture("jobposting_page.html")),
            ("/careers/blocked", 403, "text/html", "Access denied".into()),
            ("/careers/plain", 200, "text/html", "<html><body>No structured data here</body></html>".into()),
        ])
    }
}
