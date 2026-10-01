//! Minimal URL handling: parse, canonicalize for dedupe, and recognize job sites.
//!
//! Canonical form: lowercase host without `www.`, https when no explicit port, no fragment, no
//! tracking parameters, sorted query, no trailing slash. Known sites get extra rewriting so the
//! same posting reached through different links collapses to one canonical URL.

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub scheme: String,
    pub host: String,
    pub port: Option<u16>,
    pub path: String,
    pub query: Option<String>,
}

/// Where a URL points, as far as job handling is concerned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Site {
    Greenhouse {
        board: String,
        id: String,
    },
    Lever {
        company: String,
        id: String,
    },
    Ashby {
        org: String,
        id: String,
    },
    /// Boards that must never be scraped. The URL is tracked and nothing more.
    TrackingOnly(&'static str),
    Other,
}

impl Site {
    /// Short name stored in `jobs.ats`.
    pub fn ats(&self) -> Option<String> {
        match self {
            Site::Greenhouse { .. } => Some("greenhouse".into()),
            Site::Lever { .. } => Some("lever".into()),
            Site::Ashby { .. } => Some("ashby".into()),
            Site::TrackingOnly(name) => Some((*name).into()),
            Site::Other => None,
        }
    }
}

pub fn parse(input: &str) -> Option<Parsed> {
    let s = input.trim();
    let (scheme, rest) = s.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let rest = rest.split('#').next().unwrap_or("");
    let (authority, path_query) = match rest.find(['/', '?']) {
        Some(i) => rest.split_at(i),
        None => (rest, ""),
    };
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => (h, Some(p.parse().ok()?)),
        _ => (authority, None),
    };
    if host.is_empty() || host.contains(char::is_whitespace) {
        return None;
    }
    let (path, query) = match path_query.split_once('?') {
        Some((p, q)) => (p, Some(q.to_string())),
        None => (path_query, None),
    };
    Some(Parsed { scheme, host: host.to_ascii_lowercase(), port, path: path.to_string(), query })
}

const TRACKING_EXACT: &[&str] = &[
    "gclid",
    "fbclid",
    "msclkid",
    "mc_cid",
    "mc_eid",
    "ref",
    "ref_src",
    "referrer",
    "refid",
    "trk",
    "trkinfo",
    "trackingid",
    "tracking_id",
    "source",
    "src",
    "gh_src",
    "lever-source",
    "lever-source[]",
    "lever-origin",
    "_hsenc",
    "_hsmi",
    "igshid",
    "campaign",
    "origin",
    "sourceid",
    "si",
];

fn is_tracking_param(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    k.starts_with("utm_") || TRACKING_EXACT.contains(&k.as_str())
}

fn query_pairs(q: &str) -> Vec<(String, Option<String>)> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (k.to_string(), Some(v.to_string())),
            None => (p.to_string(), None),
        })
        .collect()
}

fn query_get<'a>(pairs: &'a [(String, Option<String>)], key: &str) -> Option<&'a str> {
    pairs.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).and_then(|(_, v)| v.as_deref())
}

fn host_is(host: &str, domain: &str) -> bool {
    host == domain || host.ends_with(&format!(".{domain}"))
}

const TRACKING_ONLY_HOSTS: &[(&str, &str)] = &[
    ("linkedin.com", "linkedin"),
    ("indeed.com", "indeed"),
    ("glassdoor.com", "glassdoor"),
    ("ziprecruiter.com", "ziprecruiter"),
    ("monster.com", "monster"),
    ("wellfound.com", "wellfound"),
    ("angel.co", "wellfound"),
    ("xing.com", "xing"),
    ("simplyhired.com", "simplyhired"),
    ("dice.com", "dice"),
    ("careerbuilder.com", "careerbuilder"),
];

fn tracking_only_site(host: &str) -> Option<&'static str> {
    TRACKING_ONLY_HOSTS.iter().find(|(d, _)| host_is(host, d)).map(|(_, n)| *n)
}

/// The last run of 6+ digits in a LinkedIn job slug such as `senior-engineer-at-acme-3812345678`.
fn linkedin_job_id(segment: &str) -> Option<String> {
    segment.split('-').rev().find(|t| t.len() >= 6 && t.chars().all(|c| c.is_ascii_digit())).map(str::to_string)
}

/// Returns the canonical URL for dedupe and id derivation.
pub fn canonicalize(input: &str) -> Result<String> {
    let trimmed = input.trim();
    // Without a scheme, `host.tld/path` is accepted; `mailto:x` and `javascript:x` are not. A colon
    // before the first slash is only fine when it introduces a port.
    let head = trimmed.split('/').next().unwrap_or("");
    let bare_scheme = head.split_once(':').is_some_and(|(_, after)| !after.starts_with(|c: char| c.is_ascii_digit()));
    if !trimmed.contains("://") && bare_scheme {
        return Err(Error::validation(format!("'{input}' is not a usable http(s) URL"))
            .hint("pass a full URL such as https://boards.greenhouse.io/acme/jobs/123"));
    }
    let with_scheme = if trimmed.contains("://") { trimmed.to_string() } else { format!("https://{trimmed}") };
    let p = parse(&with_scheme).ok_or_else(|| {
        Error::validation(format!("'{input}' is not a usable http(s) URL"))
            .hint("pass a full URL such as https://boards.greenhouse.io/acme/jobs/123")
    })?;

    let mut host = p.host.strip_prefix("www.").unwrap_or(&p.host).to_string();
    let segments: Vec<&str> = p.path.split('/').filter(|s| !s.is_empty()).collect();
    let pairs = p.query.as_deref().map(query_pairs).unwrap_or_default();

    // Site-specific canonical forms first.
    if host_is(&host, "linkedin.com") {
        let id = match segments.as_slice() {
            ["jobs", "view", seg, ..] => linkedin_job_id(seg),
            _ => query_get(&pairs, "currentJobId").map(str::to_string),
        };
        if let Some(id) = id {
            return Ok(format!("https://www.linkedin.com/jobs/view/{id}"));
        }
        host = "www.linkedin.com".into();
    } else if host_is(&host, "indeed.com") {
        let jk = query_get(&pairs, "jk").or_else(|| query_get(&pairs, "vjk"));
        if let Some(jk) = jk {
            return Ok(format!("https://{host}/viewjob?jk={jk}"));
        }
    } else if host == "boards.greenhouse.io" || host == "job-boards.greenhouse.io" {
        match segments.as_slice() {
            [board, "jobs", id, ..] => return Ok(format!("https://boards.greenhouse.io/{board}/jobs/{id}")),
            ["embed", "job_app"] => {
                if let (Some(board), Some(id)) = (query_get(&pairs, "for"), query_get(&pairs, "token")) {
                    return Ok(format!("https://boards.greenhouse.io/{board}/jobs/{id}"));
                }
            }
            _ => {}
        }
    } else if host == "boards.eu.greenhouse.io" || host == "job-boards.eu.greenhouse.io" {
        if let [board, "jobs", id, ..] = segments.as_slice() {
            return Ok(format!("https://boards.eu.greenhouse.io/{board}/jobs/{id}"));
        }
    } else if matches!(host.as_str(), "jobs.lever.co" | "jobs.eu.lever.co" | "jobs.ashbyhq.com") {
        // Both use /<account>/<posting-id>, with an optional /apply or /application tail.
        if let [account, id, ..] = segments.as_slice() {
            return Ok(format!("https://{host}/{account}/{id}"));
        }
    }

    // Generic form.
    let scheme = if p.port.is_none() || matches!((p.scheme.as_str(), p.port), ("https", Some(443)) | ("http", Some(80)))
    {
        "https"
    } else {
        p.scheme.as_str()
    };
    let port = match p.port {
        Some(80) | Some(443) | None => String::new(),
        Some(n) => format!(":{n}"),
    };
    let path = if segments.is_empty() { String::new() } else { format!("/{}", segments.join("/")) };
    let mut kept: Vec<(String, Option<String>)> = pairs.into_iter().filter(|(k, _)| !is_tracking_param(k)).collect();
    kept.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    let query = if kept.is_empty() {
        String::new()
    } else {
        let joined: Vec<String> = kept
            .into_iter()
            .map(|(k, v)| match v {
                Some(v) => format!("{k}={v}"),
                None => k,
            })
            .collect();
        format!("?{}", joined.join("&"))
    };
    Ok(format!("{scheme}://{host}{port}{path}{query}"))
}

/// Recognizes known job sites from a canonical URL.
pub fn classify(canonical: &str) -> Site {
    let Some(p) = parse(canonical) else { return Site::Other };
    let segments: Vec<&str> = p.path.split('/').filter(|s| !s.is_empty()).collect();
    if let Some(name) = tracking_only_site(&p.host) {
        return Site::TrackingOnly(name);
    }
    match (p.host.as_str(), segments.as_slice()) {
        ("boards.greenhouse.io" | "boards.eu.greenhouse.io", [board, "jobs", id]) => {
            Site::Greenhouse { board: (*board).into(), id: (*id).into() }
        }
        ("jobs.lever.co" | "jobs.eu.lever.co", [company, id]) => {
            Site::Lever { company: (*company).into(), id: (*id).into() }
        }
        ("jobs.ashbyhq.com", [org, id]) => Site::Ashby { org: (*org).into(), id: (*id).into() },
        _ => Site::Other,
    }
}

/// Host part of a URL, for politeness delays and domain matching.
pub fn host_of(url: &str) -> Option<String> {
    parse(url).map(|p| p.host)
}

/// The registrable-ish label of a host: `careers.acme.co.uk` -> `acme`. Good enough for matching
/// an email domain against a company name; not a public-suffix-list implementation.
pub fn domain_label(host: &str) -> String {
    let parts: Vec<&str> = host.split('.').filter(|p| !p.is_empty()).collect();
    let second_level = ["co", "com", "org", "net", "ac", "gov", "edu"];
    match parts.len() {
        0 => String::new(),
        1 => parts[0].to_string(),
        n => {
            let tld_len = if n >= 3 && parts[n - 1].len() == 2 && second_level.contains(&parts[n - 2]) { 2 } else { 1 };
            parts[n - tld_len - 1].to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canon(s: &str) -> String {
        canonicalize(s).unwrap()
    }

    #[test]
    fn strips_tracking_fragment_and_trailing_slash() {
        assert_eq!(
            canon("HTTP://www.Example.com/careers/42/?utm_source=x&b=2&a=1&gclid=zzz#apply"),
            "https://example.com/careers/42?a=1&b=2"
        );
        assert_eq!(canon("https://example.com/"), "https://example.com");
        assert_eq!(canon("example.com/jobs/1"), "https://example.com/jobs/1");
    }

    #[test]
    fn keeps_meaningful_params_and_ports() {
        assert_eq!(canon("https://acme.com/careers?gh_jid=123&gh_src=abc"), "https://acme.com/careers?gh_jid=123");
        assert_eq!(canon("http://127.0.0.1:8080/job?id=5"), "http://127.0.0.1:8080/job?id=5");
        assert_eq!(canon("https://acme.com:443/x"), "https://acme.com/x");
    }

    #[test]
    fn linkedin_variants_collapse() {
        let want = "https://www.linkedin.com/jobs/view/3812345678";
        assert_eq!(canon("https://www.linkedin.com/jobs/view/3812345678/?trackingId=abc&refId=def"), want);
        assert_eq!(
            canon("https://fi.linkedin.com/jobs/view/senior-rust-engineer-at-acme-3812345678?position=1&pageNum=0"),
            want
        );
        assert_eq!(canon("https://www.linkedin.com/jobs/search/?currentJobId=3812345678&keywords=rust"), want);
        assert_eq!(canon("https://www.linkedin.com/jobs/collections/recommended/?currentJobId=3812345678"), want);
    }

    #[test]
    fn indeed_collapses_to_jk() {
        assert_eq!(
            canon("https://fi.indeed.com/viewjob?jk=abc123&from=serp&vjs=3"),
            "https://fi.indeed.com/viewjob?jk=abc123"
        );
        assert_eq!(canon("https://www.indeed.com/jobs?q=rust&vjk=zzz9"), "https://indeed.com/viewjob?jk=zzz9");
    }

    #[test]
    fn ats_variants_collapse() {
        assert_eq!(
            canon("https://job-boards.greenhouse.io/acme/jobs/4012345?gh_src=x"),
            "https://boards.greenhouse.io/acme/jobs/4012345"
        );
        assert_eq!(
            canon("https://boards.greenhouse.io/embed/job_app?for=acme&token=4012345"),
            "https://boards.greenhouse.io/acme/jobs/4012345"
        );
        assert_eq!(
            canon("https://jobs.lever.co/acme/0f1e2d3c-aaaa-bbbb-cccc-1234567890ab/apply?lever-source=x"),
            "https://jobs.lever.co/acme/0f1e2d3c-aaaa-bbbb-cccc-1234567890ab"
        );
        assert_eq!(
            canon("https://jobs.ashbyhq.com/acme/1111-2222/application"),
            "https://jobs.ashbyhq.com/acme/1111-2222"
        );
    }

    #[test]
    fn rejects_non_http() {
        assert!(canonicalize("ftp://example.com/x").is_err());
        assert!(canonicalize("not a url").is_err());
        assert!(canonicalize("mailto:a@b.com").is_err());
    }

    #[test]
    fn classifies_sites() {
        assert_eq!(
            classify("https://boards.greenhouse.io/acme/jobs/4012345"),
            Site::Greenhouse { board: "acme".into(), id: "4012345".into() }
        );
        assert_eq!(
            classify("https://jobs.lever.co/acme/abc"),
            Site::Lever { company: "acme".into(), id: "abc".into() }
        );
        assert_eq!(classify("https://jobs.ashbyhq.com/acme/xyz"), Site::Ashby { org: "acme".into(), id: "xyz".into() });
        assert_eq!(classify("https://www.linkedin.com/jobs/view/3812345678"), Site::TrackingOnly("linkedin"));
        assert_eq!(classify("https://fi.indeed.com/viewjob?jk=abc"), Site::TrackingOnly("indeed"));
        assert_eq!(classify("https://acme.com/careers/1"), Site::Other);
    }

    #[test]
    fn domain_labels() {
        assert_eq!(domain_label("careers.acme.com"), "acme");
        assert_eq!(domain_label("mail.acme.co.uk"), "acme");
        assert_eq!(domain_label("acme.io"), "acme");
        assert_eq!(domain_label("localhost"), "localhost");
    }
}
