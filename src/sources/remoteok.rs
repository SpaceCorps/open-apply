//! RemoteOK public API: `GET /api` (optionally `?tag=<tag>`). The first array element is a legal
//! notice, not a job. RemoteOK asks API users to link back to the posting, which is why the
//! stored URL is always the posting's own page.

use serde_json::Value;

use super::{JobSource, Posting, base, str_field, timestamp_field, upstream_json};
use crate::error::{Error, Result};
use crate::util;

pub struct RemoteOk {
    pub tag: String,
}

impl JobSource for RemoteOk {
    fn label(&self) -> String {
        if self.tag.is_empty() { "remoteok".into() } else { format!("remoteok:{}", self.tag) }
    }

    fn endpoint(&self) -> String {
        let root = base("OPEN_APPLY_REMOTEOK_URL", "https://remoteok.com");
        if self.tag.is_empty() { format!("{root}/api") } else { format!("{root}/api?tag={}", self.tag) }
    }

    fn parse(&self, body: &str) -> Result<Vec<Posting>> {
        parse_jobs(&self.label(), body)
    }
}

pub fn parse_jobs(label: &str, body: &str) -> Result<Vec<Posting>> {
    let v = upstream_json(label, body)?;
    let list = v.as_array().ok_or_else(|| Error::network(format!("{label}: expected a list")))?;
    Ok(list.iter().filter_map(|j| posting_from(label, j)).collect())
}

fn posting_from(label: &str, j: &Value) -> Option<Posting> {
    if j.get("legal").is_some() {
        return None;
    }
    let title = str_field(j, "position");
    let url = str_field(j, "url");
    if title.is_empty() || url.is_empty() {
        return None;
    }
    let location = str_field(j, "location");
    let tags = j
        .get("tags")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    Some(Posting {
        source: label.to_string(),
        url,
        title,
        company: str_field(j, "company"),
        location: if location.is_empty() { "Remote".into() } else { location },
        remote: Some(true),
        description: util::html_to_text(&str_field(j, "description")),
        posted_at: timestamp_field(j, "date").or_else(|| timestamp_field(j, "epoch")),
        tags,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = include_str!("../../tests/fixtures/remoteok_api.json");

    #[test]
    fn skips_legal_notice_and_parses_jobs() {
        let jobs = parse_jobs("remoteok", FEED).unwrap();
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].title, "Senior Rust Engineer");
        assert_eq!(jobs[0].company, "Acme");
        assert_eq!(jobs[0].remote, Some(true));
        assert_eq!(jobs[0].tags, ["rust", "backend"]);
        assert_eq!(jobs[0].posted_at.as_deref(), Some("2026-09-22T10:00:00Z"));
        assert!(jobs[0].url.starts_with("https://remoteOK.com/remote-jobs/"));
        assert_eq!(jobs[1].location, "Remote");
    }

    #[test]
    fn endpoint_with_tag() {
        let s = RemoteOk { tag: "rust".into() };
        assert!(s.endpoint().ends_with("/api?tag=rust"));
        assert_eq!(s.label(), "remoteok:rust");
    }
}
