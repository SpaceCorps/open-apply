//! Reads a schema.org `JobPosting` out of a page's `application/ld+json` blocks.
//! Plenty of company career pages publish one so search engines can list the job.

use serde_json::Value;

use super::Posting;
use crate::util;

/// Returns the first JobPosting found in `html`. `page_url` is the fallback posting URL.
pub fn extract(html: &str, page_url: &str) -> Option<Posting> {
    for block in ld_json_blocks(html) {
        if let Ok(v) = serde_json::from_str::<Value>(&block)
            && let Some(job) = find_job_posting(&v)
        {
            return Some(posting_from(job, page_url));
        }
    }
    None
}

fn ld_json_blocks(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut blocks = Vec::new();
    let mut from = 0;
    while let Some(i) = lower[from..].find("<script") {
        let tag_start = from + i;
        let Some(gt) = lower[tag_start..].find('>') else { break };
        let open_end = tag_start + gt + 1;
        let open_tag = &lower[tag_start..open_end];
        let Some(close) = lower[open_end..].find("</script") else { break };
        if open_tag.contains("ld+json") {
            blocks.push(html[open_end..open_end + close].trim().to_string());
        }
        from = open_end + close;
    }
    blocks
}

fn is_job_posting(v: &Value) -> bool {
    match v.get("@type") {
        Some(Value::String(s)) => s == "JobPosting",
        Some(Value::Array(a)) => a.iter().any(|t| t.as_str() == Some("JobPosting")),
        _ => false,
    }
}

fn find_job_posting(v: &Value) -> Option<&Value> {
    match v {
        Value::Array(a) => a.iter().find_map(find_job_posting),
        Value::Object(_) if is_job_posting(v) => Some(v),
        Value::Object(_) => v.get("@graph").and_then(find_job_posting),
        _ => None,
    }
}

fn text_of(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(o @ Value::Object(_)) => text_of(o.get("name")),
        _ => String::new(),
    }
}

fn location_of(j: &Value) -> String {
    let places: Vec<&Value> = match j.get("jobLocation") {
        Some(Value::Array(a)) => a.iter().collect(),
        Some(o @ Value::Object(_)) => vec![o],
        _ => Vec::new(),
    };
    let mut parts: Vec<String> = places
        .iter()
        .map(|p| {
            let addr = p.get("address").unwrap_or(p);
            if let Some(s) = addr.as_str() {
                return s.trim().to_string();
            }
            ["addressLocality", "addressRegion", "addressCountry"]
                .iter()
                .map(|k| text_of(addr.get(*k)))
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        let req = text_of(j.get("applicantLocationRequirements"));
        if !req.is_empty() {
            parts.push(req);
        }
    }
    parts.join("; ")
}

fn posting_from(j: &Value, page_url: &str) -> Posting {
    let raw_desc = text_of(j.get("description"));
    // Descriptions arrive as HTML, sometimes entity-escaped a second time.
    let html =
        if raw_desc.contains("&lt;") && !raw_desc.contains('<') { util::decode_entities(&raw_desc) } else { raw_desc };
    let remote = match j.get("jobLocationType").and_then(Value::as_str) {
        Some(t) if t.eq_ignore_ascii_case("TELECOMMUTE") => Some(true),
        _ => None,
    };
    let url = Some(text_of(j.get("url"))).filter(|u| u.starts_with("http")).unwrap_or_else(|| page_url.to_string());
    Posting {
        source: "jsonld".into(),
        url,
        title: util::decode_entities(&text_of(j.get("title"))),
        company: util::decode_entities(&text_of(j.get("hiringOrganization"))),
        location: location_of(j),
        remote,
        description: util::html_to_text(&html),
        posted_at: j
            .get("datePosted")
            .and_then(Value::as_str)
            .and_then(util::parse_timestamp)
            .map(util::format_rfc3339),
        tags: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = include_str!("../../tests/fixtures/jobposting_page.html");

    #[test]
    fn extracts_job_posting_from_graph() {
        let p = extract(PAGE, "https://careers.example.org/jobs/42").unwrap();
        assert_eq!(p.title, "Data Engineer");
        assert_eq!(p.company, "Example Oy");
        assert_eq!(p.location, "Helsinki, Uusimaa, FI");
        assert_eq!(p.remote, None);
        assert_eq!(p.posted_at.as_deref(), Some("2026-09-05T00:00:00Z"));
        assert!(p.description.contains("Own the data platform"));
        assert!(p.description.contains("- SQL"));
        assert_eq!(p.url, "https://careers.example.org/jobs/42");
    }

    #[test]
    fn handles_plain_object_array_location_and_telecommute() {
        let html = r#"<script type="application/ld+json">{"@context":"https://schema.org","@type":"JobPosting","title":"Dev","hiringOrganization":"Solo Ltd","jobLocationType":"TELECOMMUTE","jobLocation":[{"address":{"addressLocality":"Oslo","addressCountry":{"name":"Norway"}}},{"address":"Remote EU"}],"description":"&lt;p&gt;Hello&lt;/p&gt;"}</script>"#;
        let p = extract(html, "https://x.example/j").unwrap();
        assert_eq!(p.company, "Solo Ltd");
        assert_eq!(p.location, "Oslo, Norway; Remote EU");
        assert_eq!(p.remote, Some(true));
        assert_eq!(p.description, "Hello");
    }

    #[test]
    fn none_when_no_job_posting() {
        assert!(
            extract("<html><script type=\"application/ld+json\">{\"@type\":\"Organization\"}</script></html>", "u")
                .is_none()
        );
        assert!(extract("<html>nothing</html>", "u").is_none());
        assert!(extract("<script type=\"application/ld+json\">{not json</script>", "u").is_none());
    }
}
