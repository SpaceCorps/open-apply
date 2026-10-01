//! Public job feeds. Each source is a small type behind `JobSource` with a pure `parse` function,
//! so tests run on recorded fixtures and never touch the network. Only documented, unauthenticated
//! endpoints are used; there is no scraping and no bot-detection workaround.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::config::Config;
use crate::error::{Error, Result};
use crate::url::{self, Site};

pub mod arbeitnow;
pub mod ashby;
pub mod greenhouse;
pub mod jsonld;
pub mod lever;
pub mod remoteok;
pub mod weworkremotely;

pub const REPO_URL: &str = "https://github.com/SpaceCorps/open-apply";

pub fn user_agent() -> String {
    format!("open-apply/{} (+{REPO_URL})", env!("CARGO_PKG_VERSION"))
}

/// A posting as a feed describes it, before it becomes a stored job.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Posting {
    /// Label of the feed it came from, for example `greenhouse:acme`.
    pub source: String,
    pub url: String,
    pub title: String,
    pub company: String,
    pub location: String,
    pub remote: Option<bool>,
    /// Plain text. Untrusted: it is third-party content.
    pub description: String,
    pub posted_at: Option<String>,
    pub tags: Vec<String>,
}

pub trait JobSource {
    /// `kind:ident`, or just `kind` for feeds without an identifier.
    fn label(&self) -> String;
    fn endpoint(&self) -> String;
    fn accept(&self) -> &'static str {
        "application/json"
    }
    /// Pure: response body in, postings out.
    fn parse(&self, body: &str) -> Result<Vec<Posting>>;
}

/// Base URL for a feed; `OPEN_APPLY_<NAME>_URL` overrides it so tests can point at a local mock.
pub fn base(env: &str, default: &str) -> String {
    std::env::var(env)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
        .trim_end_matches('/')
        .to_string()
}

/// Parses an upstream JSON body, mapping failure to a network-class error (the upstream misbehaved).
pub fn upstream_json(label: &str, body: &str) -> Result<Value> {
    serde_json::from_str(body).map_err(|e| Error::network(format!("{label}: response is not valid JSON ({e})")))
}

pub fn str_field(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").trim().to_string()
}

/// Normalizes any timestamp-looking value (ISO string or epoch seconds or millis) to RFC 3339.
pub fn timestamp_field(v: &Value, key: &str) -> Option<String> {
    let f = v.get(key)?;
    if let Some(s) = f.as_str() {
        return crate::util::parse_timestamp(s).map(crate::util::format_rfc3339);
    }
    let n = f.as_i64()?;
    let secs = if n > 100_000_000_000 { n / 1000 } else { n };
    Some(crate::util::format_rfc3339(secs))
}

// ---------------------------------------------------------------------------------------------
// Source specs: `greenhouse:acme`, `remoteok`, ...
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSpec {
    pub kind: String,
    pub ident: String,
}

const NEEDS_IDENT: [&str; 3] = ["greenhouse", "lever", "ashby"];
const OPTIONAL_IDENT: [&str; 2] = ["remoteok", "weworkremotely"];
const NO_IDENT: [&str; 1] = ["arbeitnow"];

impl SourceSpec {
    pub fn parse(input: &str) -> Result<SourceSpec> {
        let s = input.trim();
        let (kind, ident) = s.split_once(':').unwrap_or((s, ""));
        let kind = kind.trim().to_ascii_lowercase();
        let ident = ident.trim().to_string();
        let known = NEEDS_IDENT.iter().chain(&OPTIONAL_IDENT).chain(&NO_IDENT).any(|k| *k == kind);
        if !known {
            return Err(Error::validation(format!("unknown source kind '{kind}'"))
                .hint("use greenhouse:<board>, lever:<company>, ashby:<org>, remoteok, weworkremotely or arbeitnow"));
        }
        if NEEDS_IDENT.contains(&kind.as_str()) && ident.is_empty() {
            return Err(Error::validation(format!("source '{kind}' needs an identifier"))
                .hint(format!("for example {kind}:acme")));
        }
        if NO_IDENT.contains(&kind.as_str()) && !ident.is_empty() {
            return Err(Error::validation(format!("source '{kind}' takes no identifier")));
        }
        if !ident.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) {
            return Err(Error::validation(format!(
                "identifier '{ident}' has characters outside letters, digits, '-', '_' and '.'"
            )));
        }
        Ok(SourceSpec { kind, ident })
    }

    pub fn label(&self) -> String {
        if self.ident.is_empty() { self.kind.clone() } else { format!("{}:{}", self.kind, self.ident) }
    }

    pub fn source(&self) -> Box<dyn JobSource> {
        let id = self.ident.clone();
        match self.kind.as_str() {
            "greenhouse" => Box::new(greenhouse::Greenhouse { board: id }),
            "lever" => Box::new(lever::Lever { company: id }),
            "ashby" => Box::new(ashby::Ashby { org: id }),
            "remoteok" => Box::new(remoteok::RemoteOk { tag: id }),
            "weworkremotely" => Box::new(weworkremotely::WeWorkRemotely { category: id }),
            _ => Box::new(arbeitnow::Arbeitnow),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------------------------

pub struct Http {
    agent: ureq::Agent,
    delay: Duration,
    last: Mutex<HashMap<String, Instant>>,
}

impl Http {
    pub fn new(cfg: &Config) -> Http {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(cfg.http_timeout_secs.max(1))))
            .user_agent(user_agent())
            .http_status_as_error(false)
            .build();
        Http {
            agent: ureq::Agent::new_with_config(config),
            delay: Duration::from_millis(cfg.request_delay_ms()),
            last: Mutex::new(HashMap::new()),
        }
    }

    /// Waits so two requests to the same host are at least `delay` apart.
    fn be_polite(&self, host: &str) {
        let wait = {
            let mut last = self.last.lock().unwrap_or_else(|p| p.into_inner());
            let now = Instant::now();
            let wait = last.get(host).map(|t| self.delay.saturating_sub(now.duration_since(*t))).unwrap_or_default();
            last.insert(host.to_string(), now + wait);
            wait
        };
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
    }

    pub fn get(&self, url: &str, accept: &str) -> Result<String> {
        let host = url::host_of(url).unwrap_or_default();
        self.be_polite(&host);
        let mut resp = self.agent.get(url).header("Accept", accept).call().map_err(|e| {
            Error::network(format!("request to {host} failed: {e}")).hint("check your connection and retry")
        })?;
        let status = resp.status().as_u16();
        match status {
            200..=299 => {}
            404 | 410 => return Err(Error::not_found(format!("{host} answered {status}: nothing at {url}"))),
            429 => {
                return Err(Error::network(format!("{host} rate limited the request (429)"))
                    .hint("wait a few minutes before searching this source again"));
            }
            401 | 403 => {
                return Err(Error::network(format!("{host} refused the request ({status})"))
                    .hint("the site does not allow automated access here; add the posting by hand with --title and --no-fetch"));
            }
            _ => return Err(Error::network(format!("{host} answered {status}")).hint("retry later")),
        }
        resp.body_mut()
            .with_config()
            .limit(64 * 1024 * 1024)
            .read_to_string()
            .map_err(|e| Error::network(format!("could not read the response from {host}: {e}")))
    }
}

pub fn fetch(http: &Http, source: &dyn JobSource) -> Result<Vec<Posting>> {
    let body = http.get(&source.endpoint(), source.accept())?;
    source.parse(&body)
}

/// Looks up a single posting on a known ATS through its public JSON endpoint.
/// `Ok(None)` for sites that have no such endpoint.
pub fn fetch_ats_posting(http: &Http, site: &Site) -> Result<Option<Posting>> {
    match site {
        Site::Greenhouse { board, id } => greenhouse::fetch_one(http, board, id).map(Some),
        Site::Lever { company, id } => lever::fetch_one(http, company, id).map(Some),
        Site::Ashby { org, id } => ashby::fetch_one(http, org, id).map(Some),
        _ => Ok(None),
    }
}

/// Fetches an arbitrary page and reads a schema.org JobPosting out of it, if there is one.
/// A blocked or failing page is reported as an error for the caller to downgrade to a warning.
pub fn fetch_page_posting(http: &Http, url: &str) -> Result<Option<Posting>> {
    let html = http.get(url, "text/html,application/xhtml+xml")?;
    Ok(jsonld::extract(&html, url))
}

// ---------------------------------------------------------------------------------------------
// Search filtering
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct Query {
    pub text: Option<String>,
    pub location: Option<String>,
    pub remote: bool,
}

impl Query {
    pub fn matches(&self, p: &Posting) -> bool {
        if let Some(text) = &self.text {
            let hay = format!("{} {} {} {}", p.title, p.company, p.tags.join(" "), p.description).to_lowercase();
            if !text.split_whitespace().all(|term| hay.contains(&term.to_lowercase())) {
                return false;
            }
        }
        if let Some(loc) = &self.location
            && !p.location.to_lowercase().contains(&loc.to_lowercase())
        {
            return false;
        }
        if self.remote {
            match p.remote {
                Some(r) => {
                    if !r {
                        return false;
                    }
                }
                None => {
                    let hay = format!("{} {}", p.location, p.title).to_lowercase();
                    if !hay.contains("remote") {
                        return false;
                    }
                }
            }
        }
        true
    }
}

/// Takes postings round-robin from each list so a small `--limit` still samples every source.
pub fn interleave(lists: Vec<Vec<Posting>>) -> Vec<Posting> {
    let mut iters: Vec<_> = lists.into_iter().map(Vec::into_iter).collect();
    let mut out = Vec::new();
    loop {
        let mut progressed = false;
        for it in &mut iters {
            if let Some(p) = it.next() {
                out.push(p);
                progressed = true;
            }
        }
        if !progressed {
            return out;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn posting(title: &str, location: &str, remote: Option<bool>) -> Posting {
        Posting { title: title.into(), location: location.into(), remote, company: "Acme".into(), ..Default::default() }
    }

    #[test]
    fn spec_parsing() {
        assert_eq!(SourceSpec::parse("Greenhouse:acme").unwrap().label(), "greenhouse:acme");
        assert_eq!(SourceSpec::parse("remoteok").unwrap().label(), "remoteok");
        assert_eq!(SourceSpec::parse("remoteok:rust").unwrap().label(), "remoteok:rust");
        assert_eq!(SourceSpec::parse("arbeitnow").unwrap().label(), "arbeitnow");
        assert!(SourceSpec::parse("greenhouse").is_err());
        assert!(SourceSpec::parse("linkedin:acme").is_err());
        assert!(SourceSpec::parse("arbeitnow:x").is_err());
        assert!(SourceSpec::parse("lever:a/b").is_err());
    }

    #[test]
    fn query_terms_location_remote() {
        let p = posting("Senior Rust Engineer", "Berlin", None);
        let q = Query { text: Some("rust engineer".into()), ..Default::default() };
        assert!(q.matches(&p));
        let q = Query { text: Some("rust python".into()), ..Default::default() };
        assert!(!q.matches(&p));
        let q = Query { location: Some("berlin".into()), ..Default::default() };
        assert!(q.matches(&p));
        let q = Query { location: Some("paris".into()), ..Default::default() };
        assert!(!q.matches(&p));
        let remote = Query { remote: true, ..Default::default() };
        assert!(!remote.matches(&p));
        assert!(remote.matches(&posting("Engineer", "Remote - EU", None)));
        assert!(remote.matches(&posting("Engineer", "Berlin", Some(true))));
        assert!(!remote.matches(&posting("Engineer", "Remote-ish office", Some(false))));
    }

    #[test]
    fn interleave_round_robin() {
        let a = vec![posting("a1", "", None), posting("a2", "", None), posting("a3", "", None)];
        let b = vec![posting("b1", "", None)];
        let titles: Vec<String> = interleave(vec![a, b]).into_iter().map(|p| p.title).collect();
        assert_eq!(titles, ["a1", "b1", "a2", "a3"]);
    }

    #[test]
    fn user_agent_is_honest() {
        let ua = user_agent();
        assert!(ua.starts_with("open-apply/"));
        assert!(ua.contains("github.com/SpaceCorps/open-apply"));
    }

    #[test]
    fn timestamps_from_iso_and_epochs() {
        let v = serde_json::json!({"a": "2026-09-20T10:11:12-04:00", "b": 1_790_000_000i64, "c": 1_790_000_000_000i64});
        assert_eq!(timestamp_field(&v, "a").as_deref(), Some("2026-09-20T14:11:12Z"));
        assert_eq!(timestamp_field(&v, "b"), timestamp_field(&v, "c"));
        assert_eq!(timestamp_field(&v, "zzz"), None);
    }
}
