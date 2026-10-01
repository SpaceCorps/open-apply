//! Greenhouse job boards API: `GET /v1/boards/{board}/jobs?content=true`.
//! `content` is HTML that has been entity-escaped once more, so it is decoded before stripping.

use serde_json::Value;

use super::{Http, JobSource, Posting, base, str_field, timestamp_field, upstream_json};
use crate::error::{Error, Result};
use crate::util;

pub struct Greenhouse {
    pub board: String,
}

fn api_base() -> String {
    base("OPEN_APPLY_GREENHOUSE_URL", "https://boards-api.greenhouse.io")
}

impl JobSource for Greenhouse {
    fn label(&self) -> String {
        format!("greenhouse:{}", self.board)
    }

    fn endpoint(&self) -> String {
        format!("{}/v1/boards/{}/jobs?content=true", api_base(), self.board)
    }

    fn parse(&self, body: &str) -> Result<Vec<Posting>> {
        parse_jobs(&self.board, body)
    }
}

pub fn parse_jobs(board: &str, body: &str) -> Result<Vec<Posting>> {
    let v = upstream_json(&format!("greenhouse:{board}"), body)?;
    let jobs = v
        .get("jobs")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::network(format!("greenhouse:{board}: response has no 'jobs' list")))?;
    Ok(jobs.iter().filter_map(|j| posting_from(board, j)).collect())
}

pub fn parse_job(board: &str, body: &str) -> Result<Posting> {
    let v = upstream_json(&format!("greenhouse:{board}"), body)?;
    posting_from(board, &v).ok_or_else(|| Error::network(format!("greenhouse:{board}: response is not a job")))
}

fn posting_from(board: &str, j: &Value) -> Option<Posting> {
    let title = str_field(j, "title");
    let url = str_field(j, "absolute_url");
    if title.is_empty() || url.is_empty() {
        return None;
    }
    let location =
        j.get("location").and_then(|l| l.get("name")).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let company =
        Some(str_field(j, "company_name")).filter(|c| !c.is_empty()).unwrap_or_else(|| util::prettify_slug(board));
    let remote = location.to_lowercase().contains("remote").then_some(true);
    let content = str_field(j, "content");
    Some(Posting {
        source: format!("greenhouse:{board}"),
        url,
        title,
        company,
        remote,
        description: util::html_to_text(&util::decode_entities(&content)),
        posted_at: timestamp_field(j, "first_published").or_else(|| timestamp_field(j, "updated_at")),
        tags: Vec::new(),
        location,
    })
}

pub fn fetch_one(http: &Http, board: &str, id: &str) -> Result<Posting> {
    let url = format!("{}/v1/boards/{board}/jobs/{id}", api_base());
    let body = http.get(&url, "application/json")?;
    parse_job(board, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = include_str!("../../tests/fixtures/greenhouse_jobs.json");

    #[test]
    fn parses_list_fixture() {
        let jobs = parse_jobs("acme-labs", LIST).unwrap();
        assert_eq!(jobs.len(), 2);
        let first = &jobs[0];
        assert_eq!(first.title, "Senior Rust Engineer");
        assert_eq!(first.company, "Acme Labs");
        assert_eq!(first.location, "Remote - Europe");
        assert_eq!(first.remote, Some(true));
        assert_eq!(first.url, "https://job-boards.greenhouse.io/acme-labs/jobs/4012345");
        assert_eq!(first.posted_at.as_deref(), Some("2026-09-01T09:00:00Z"));
        assert!(first.description.contains("- Design the sync engine"), "{}", first.description);
        assert!(!first.description.contains('<'));
        assert_eq!(first.source, "greenhouse:acme-labs");
        assert_eq!(jobs[1].remote, None);
    }

    #[test]
    fn single_job_uses_company_name_when_given() {
        let body = r#"{"id":1,"title":"Engineer","absolute_url":"https://boards.greenhouse.io/acme/jobs/1","company_name":"Acme Robotics","location":{"name":"Berlin"},"content":"&lt;p&gt;Hi&lt;/p&gt;"}"#;
        let p = parse_job("acme", body).unwrap();
        assert_eq!(p.company, "Acme Robotics");
        assert_eq!(p.description, "Hi");
    }

    #[test]
    fn bad_bodies_are_upstream_errors() {
        assert_eq!(parse_jobs("x", "<html>").unwrap_err().code, crate::error::ErrorCode::Network);
        assert_eq!(parse_jobs("x", "{}").unwrap_err().code, crate::error::ErrorCode::Network);
    }
}
