//! Lever postings API: `GET /v0/postings/{company}?mode=json`.

use serde_json::Value;

use super::{Http, JobSource, Posting, base, str_field, timestamp_field, upstream_json};
use crate::error::{Error, Result};
use crate::util;

pub struct Lever {
    pub company: String,
}

fn api_base() -> String {
    base("OPEN_APPLY_LEVER_URL", "https://api.lever.co")
}

impl JobSource for Lever {
    fn label(&self) -> String {
        format!("lever:{}", self.company)
    }

    fn endpoint(&self) -> String {
        format!("{}/v0/postings/{}?mode=json", api_base(), self.company)
    }

    fn parse(&self, body: &str) -> Result<Vec<Posting>> {
        parse_postings(&self.company, body)
    }
}

pub fn parse_postings(company: &str, body: &str) -> Result<Vec<Posting>> {
    let v = upstream_json(&format!("lever:{company}"), body)?;
    let list = v.as_array().ok_or_else(|| Error::network(format!("lever:{company}: expected a list of postings")))?;
    Ok(list.iter().filter_map(|p| posting_from(company, p)).collect())
}

pub fn parse_posting(company: &str, body: &str) -> Result<Posting> {
    let v = upstream_json(&format!("lever:{company}"), body)?;
    posting_from(company, &v).ok_or_else(|| Error::network(format!("lever:{company}: response is not a posting")))
}

fn posting_from(company: &str, p: &Value) -> Option<Posting> {
    let title = str_field(p, "text");
    let url = str_field(p, "hostedUrl");
    if title.is_empty() || url.is_empty() {
        return None;
    }
    let cats = p.get("categories").cloned().unwrap_or(Value::Null);
    let all_locations: Vec<String> = cats
        .get("allLocations")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let location = if all_locations.is_empty() { str_field(&cats, "location") } else { all_locations.join("; ") };
    let remote = match str_field(p, "workplaceType").to_lowercase().as_str() {
        "remote" => Some(true),
        "" => None,
        _ => Some(false),
    };

    // Lever splits the body into an intro, titled lists, and a closing block.
    let mut text = Vec::new();
    let intro = str_field(p, "descriptionPlain");
    text.push(if intro.is_empty() { util::html_to_text(&str_field(p, "description")) } else { intro });
    if let Some(lists) = p.get("lists").and_then(Value::as_array) {
        for l in lists {
            text.push(format!("{}\n{}", str_field(l, "text"), util::html_to_text(&str_field(l, "content"))));
        }
    }
    let closing = str_field(p, "additionalPlain");
    text.push(if closing.is_empty() { util::html_to_text(&str_field(p, "additional")) } else { closing });

    let tags =
        ["department", "team", "commitment"].iter().map(|k| str_field(&cats, k)).filter(|s| !s.is_empty()).collect();

    Some(Posting {
        source: format!("lever:{company}"),
        url,
        title,
        company: util::prettify_slug(company),
        location,
        remote,
        description: util::collapse_text(&text.join("\n\n")),
        posted_at: timestamp_field(p, "createdAt"),
        tags,
    })
}

pub fn fetch_one(http: &Http, company: &str, id: &str) -> Result<Posting> {
    let url = format!("{}/v0/postings/{company}/{id}", api_base());
    let body = http.get(&url, "application/json")?;
    parse_posting(company, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = include_str!("../../tests/fixtures/lever_postings.json");

    #[test]
    fn parses_fixture() {
        let posts = parse_postings("acme", LIST).unwrap();
        assert_eq!(posts.len(), 2);
        let p = &posts[0];
        assert_eq!(p.title, "Platform Engineer");
        assert_eq!(p.company, "Acme");
        assert_eq!(p.location, "Remote - US; Toronto");
        assert_eq!(p.remote, Some(true));
        assert_eq!(p.url, "https://jobs.lever.co/acme/0f1e2d3c-aaaa-bbbb-cccc-1234567890ab");
        assert!(p.description.contains("Build the deploy pipeline"));
        assert!(p.description.contains("- Kubernetes experience"), "{}", p.description);
        assert!(p.description.contains("We are an equal opportunity employer"));
        assert_eq!(p.tags, ["Engineering", "Platform", "Full-time"]);
        assert!(p.posted_at.is_some());
        assert_eq!(posts[1].remote, Some(false));
    }

    #[test]
    fn single_posting_object() {
        let body = r#"{"id":"x","text":"Dev","hostedUrl":"https://jobs.lever.co/acme/x","categories":{"location":"Oslo"},"descriptionPlain":"Hello"}"#;
        let p = parse_posting("acme", body).unwrap();
        assert_eq!((p.title.as_str(), p.location.as_str(), p.remote), ("Dev", "Oslo", None));
    }

    #[test]
    fn non_list_is_upstream_error() {
        assert!(parse_postings("x", r#"{"ok":false,"error":"Document not found"}"#).is_err());
    }
}
