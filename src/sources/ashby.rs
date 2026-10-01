//! Ashby public job board API: `GET /posting-api/job-board/{org}`.
//! There is no single-posting endpoint, so a lookup fetches the board and picks the job by id.

use serde_json::Value;

use super::{Http, JobSource, Posting, base, str_field, timestamp_field, upstream_json};
use crate::error::{Error, Result};
use crate::util;

pub struct Ashby {
    pub org: String,
}

fn api_base() -> String {
    base("OPEN_APPLY_ASHBY_URL", "https://api.ashbyhq.com")
}

impl JobSource for Ashby {
    fn label(&self) -> String {
        format!("ashby:{}", self.org)
    }

    fn endpoint(&self) -> String {
        format!("{}/posting-api/job-board/{}", api_base(), self.org)
    }

    fn parse(&self, body: &str) -> Result<Vec<Posting>> {
        parse_board(&self.org, body)
    }
}

pub fn parse_board(org: &str, body: &str) -> Result<Vec<Posting>> {
    let v = upstream_json(&format!("ashby:{org}"), body)?;
    let jobs = v
        .get("jobs")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::network(format!("ashby:{org}: response has no 'jobs' list")))?;
    Ok(jobs.iter().filter_map(|j| posting_from(org, j)).collect())
}

fn posting_from(org: &str, j: &Value) -> Option<Posting> {
    if j.get("isListed").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    let title = str_field(j, "title");
    let url = str_field(j, "jobUrl");
    if title.is_empty() || url.is_empty() {
        return None;
    }
    let remote = j.get("isRemote").and_then(Value::as_bool).or_else(|| {
        match str_field(j, "workplaceType").to_lowercase().as_str() {
            "remote" => Some(true),
            "" => None,
            _ => Some(false),
        }
    });
    let plain = str_field(j, "descriptionPlain");
    let description = if plain.is_empty() { util::html_to_text(&str_field(j, "descriptionHtml")) } else { plain };
    let tags =
        ["department", "team", "employmentType"].iter().map(|k| str_field(j, k)).filter(|s| !s.is_empty()).collect();
    Some(Posting {
        source: format!("ashby:{org}"),
        url,
        title,
        company: util::prettify_slug(org),
        location: str_field(j, "location"),
        remote,
        description,
        posted_at: timestamp_field(j, "publishedAt"),
        tags,
    })
}

pub fn fetch_one(http: &Http, org: &str, id: &str) -> Result<Posting> {
    let url = format!("{}/posting-api/job-board/{org}", api_base());
    let body = http.get(&url, "application/json")?;
    find_in_board(org, id, &body)
}

/// Pure half of `fetch_one`: pick the posting whose URL or id carries `id`.
pub fn find_in_board(org: &str, id: &str, body: &str) -> Result<Posting> {
    parse_board(org, body)?.into_iter().find(|p| p.url.contains(id)).ok_or_else(|| {
        Error::not_found(format!("ashby:{org} has no listed posting {id}")).hint("the posting may have been closed")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOARD: &str = include_str!("../../tests/fixtures/ashby_board.json");

    #[test]
    fn parses_fixture_and_skips_unlisted() {
        let jobs = parse_board("acme", BOARD).unwrap();
        assert_eq!(jobs.len(), 2);
        let p = &jobs[0];
        assert_eq!(p.title, "Staff Backend Engineer");
        assert_eq!(p.location, "Berlin, Germany");
        assert_eq!(p.remote, Some(true));
        assert_eq!(p.posted_at.as_deref(), Some("2026-09-10T08:00:00Z"));
        assert!(p.description.contains("ship reliable systems"));
        assert_eq!(p.tags, ["Engineering", "Platform", "FullTime"]);
        assert_eq!(jobs[1].remote, Some(false));
    }

    #[test]
    fn finds_by_id() {
        let p = find_in_board("acme", "11111111-2222-3333-4444-555555555555", BOARD).unwrap();
        assert_eq!(p.title, "Staff Backend Engineer");
        let err = find_in_board("acme", "nope", BOARD).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::NotFound);
    }
}
