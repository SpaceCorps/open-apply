//! We Work Remotely RSS: `/remote-jobs.rss`, or `/categories/<category>.rss`.
//! The feed is small and regular, so a few string scans replace an XML dependency.

use super::{JobSource, Posting, base};
use crate::error::{Error, Result};
use crate::util;

pub struct WeWorkRemotely {
    pub category: String,
}

impl JobSource for WeWorkRemotely {
    fn label(&self) -> String {
        if self.category.is_empty() { "weworkremotely".into() } else { format!("weworkremotely:{}", self.category) }
    }

    fn endpoint(&self) -> String {
        let root = base("OPEN_APPLY_WWR_URL", "https://weworkremotely.com");
        if self.category.is_empty() {
            format!("{root}/remote-jobs.rss")
        } else {
            format!("{root}/categories/{}.rss", self.category)
        }
    }

    fn accept(&self) -> &'static str {
        "application/rss+xml, application/xml, text/xml"
    }

    fn parse(&self, body: &str) -> Result<Vec<Posting>> {
        parse_rss(&self.label(), body)
    }
}

/// Text of the first `<tag>...</tag>` in `xml`, with CDATA unwrapped and entities decoded.
fn tag_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let mut from = 0;
    while let Some(i) = xml[from..].find(&open) {
        let start = from + i + open.len();
        // Make sure we matched the whole tag name (`<link>` and not `<linkedin>`).
        match xml[start..].chars().next() {
            Some('>') | Some(' ') | Some('/') => {}
            _ => {
                from = start;
                continue;
            }
        }
        let gt = xml[start..].find('>')? + start;
        if xml[..gt].ends_with('/') {
            return Some(String::new());
        }
        let close = format!("</{tag}>");
        let end = xml[gt + 1..].find(&close)? + gt + 1;
        let inner = xml[gt + 1..end].trim();
        return Some(match inner.strip_prefix("<![CDATA[").and_then(|s| s.strip_suffix("]]>")) {
            Some(cdata) => cdata.to_string(),
            None => util::decode_entities(inner),
        });
    }
    None
}

pub fn parse_rss(label: &str, body: &str) -> Result<Vec<Posting>> {
    if !body.contains("<rss") && !body.contains("<channel") {
        return Err(Error::network(format!("{label}: response is not an RSS feed")));
    }
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find("<item>") {
        let after = &rest[start + 6..];
        let Some(end) = after.find("</item>") else { break };
        let item = &after[..end];
        rest = &after[end + 7..];

        let title_raw = tag_text(item, "title").unwrap_or_default();
        let url =
            tag_text(item, "link").filter(|l| !l.is_empty()).or_else(|| tag_text(item, "guid")).unwrap_or_default();
        if title_raw.is_empty() || url.is_empty() {
            continue;
        }
        // "Company: Job title"
        let (company, title) = match title_raw.split_once(": ") {
            Some((c, t)) => (c.trim().to_string(), t.trim().to_string()),
            None => (String::new(), title_raw.trim().to_string()),
        };
        let region = tag_text(item, "region").unwrap_or_default();
        let country = tag_text(item, "country").unwrap_or_default();
        let location =
            [region, country].iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ");
        let tags = ["category", "type"]
            .iter()
            .filter_map(|t| tag_text(item, t))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        out.push(Posting {
            source: label.to_string(),
            url: url.trim().to_string(),
            title,
            company,
            location: if location.is_empty() { "Remote".into() } else { location },
            remote: Some(true),
            description: util::html_to_text(&tag_text(item, "description").unwrap_or_default()),
            posted_at: tag_text(item, "pubDate").and_then(|d| util::parse_rfc2822(&d)).map(util::format_rfc3339),
            tags,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = include_str!("../../tests/fixtures/weworkremotely.rss");

    #[test]
    fn parses_items() {
        let jobs = parse_rss("weworkremotely", FEED).unwrap();
        assert_eq!(jobs.len(), 2);
        let p = &jobs[0];
        assert_eq!(p.company, "Acme");
        assert_eq!(p.title, "Senior Rust Engineer");
        assert_eq!(p.location, "Anywhere in the World");
        assert_eq!(p.url, "https://weworkremotely.com/remote-jobs/acme-senior-rust-engineer");
        assert_eq!(p.posted_at.as_deref(), Some("2026-09-22T10:00:00Z"));
        assert!(p.description.contains("Join our small team"));
        assert!(!p.description.contains('<'));
        assert_eq!(p.tags, ["Back-End Programming", "Full-Time"]);
        assert_eq!(jobs[1].company, "Globex & Sons");
    }

    #[test]
    fn rejects_non_rss() {
        assert!(parse_rss("x", "<html>blocked</html>").is_err());
    }

    #[test]
    fn tag_name_must_match_exactly() {
        assert_eq!(tag_text("<linkedin>no</linkedin><link>yes</link>", "link").as_deref(), Some("yes"));
    }
}
