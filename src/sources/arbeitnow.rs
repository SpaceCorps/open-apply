//! Arbeitnow job board API: `GET /api/job-board-api`.

use serde_json::Value;

use super::{JobSource, Posting, base, str_field, timestamp_field, upstream_json};
use crate::error::{Error, Result};
use crate::util;

pub struct Arbeitnow;

impl JobSource for Arbeitnow {
    fn label(&self) -> String {
        "arbeitnow".into()
    }

    fn endpoint(&self) -> String {
        format!("{}/api/job-board-api", base("OPEN_APPLY_ARBEITNOW_URL", "https://www.arbeitnow.com"))
    }

    fn parse(&self, body: &str) -> Result<Vec<Posting>> {
        parse_jobs(body)
    }
}

pub fn parse_jobs(body: &str) -> Result<Vec<Posting>> {
    let v = upstream_json("arbeitnow", body)?;
    let data = v
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::network("arbeitnow: response has no 'data' list"))?;
    Ok(data
        .iter()
        .filter_map(|j| {
            let title = str_field(j, "title");
            let url = str_field(j, "url");
            if title.is_empty() || url.is_empty() {
                return None;
            }
            let mut tags: Vec<String> = ["tags", "job_types"]
                .iter()
                .filter_map(|k| j.get(*k).and_then(Value::as_array))
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
            tags.dedup();
            Some(Posting {
                source: "arbeitnow".into(),
                url,
                title,
                company: str_field(j, "company_name"),
                location: str_field(j, "location"),
                remote: j.get("remote").and_then(Value::as_bool),
                description: util::html_to_text(&str_field(j, "description")),
                posted_at: timestamp_field(j, "created_at"),
                tags,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = include_str!("../../tests/fixtures/arbeitnow_api.json");

    #[test]
    fn parses_fixture() {
        let jobs = parse_jobs(FEED).unwrap();
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].company, "Acme GmbH");
        assert_eq!(jobs[0].location, "Berlin");
        assert_eq!(jobs[0].remote, Some(false));
        assert_eq!(jobs[1].remote, Some(true));
        assert_eq!(jobs[0].posted_at.as_deref(), Some("2026-09-18T12:00:00Z"));
        assert_eq!(jobs[0].tags, ["Engineering", "full time"]);
        assert!(jobs[0].description.starts_with("Build"));
    }

    #[test]
    fn missing_data_is_upstream_error() {
        assert!(parse_jobs(r#"{"message":"nope"}"#).is_err());
    }
}
