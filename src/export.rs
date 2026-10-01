//! Export and import. JSON is lossless (jobs with their full event history); CSV carries one
//! row per job for spreadsheets; Markdown is a readable table. Import reads JSON or CSV.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::db;
use crate::error::{Error, Result};
use crate::model::{Event, EventType, Job, Status};
use crate::url;
use crate::util;

pub const FORMAT_NAME: &str = "open-apply-export";
pub const FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
pub struct JobWithEvents {
    #[serde(flatten)]
    pub job: Job,
    #[serde(default)]
    pub events: Vec<EventRecord>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct EventRecord {
    #[serde(rename = "type")]
    pub kind: EventType,
    pub status: Option<Status>,
    pub note: Option<String>,
    pub at: String,
    pub source: String,
    pub recorded_at: Option<String>,
}

impl From<&Event> for EventRecord {
    fn from(e: &Event) -> Self {
        EventRecord {
            kind: e.kind,
            status: e.status,
            note: e.note.clone(),
            at: e.at.clone(),
            source: e.source.clone(),
            recorded_at: Some(e.recorded_at.clone()),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Export {
    pub format: String,
    pub version: u32,
    pub exported_at: String,
    pub jobs: Vec<JobWithEvents>,
}

pub fn build(jobs: Vec<Job>, events: Vec<Event>) -> Export {
    let jobs = jobs
        .into_iter()
        .map(|job| {
            let evs = events.iter().filter(|e| e.job_id == job.id).map(EventRecord::from).collect();
            JobWithEvents { job, events: evs }
        })
        .collect();
    Export { format: FORMAT_NAME.into(), version: FORMAT_VERSION, exported_at: util::now_rfc3339(), jobs }
}

pub fn to_json(export: &Export) -> Result<String> {
    Ok(serde_json::to_string_pretty(export)?)
}

pub const CSV_COLUMNS: [&str; 13] = [
    "id",
    "status",
    "title",
    "company",
    "location",
    "source",
    "url",
    "applied_via",
    "applied_at",
    "posted_at",
    "created_at",
    "updated_at",
    "notes",
];

fn csv_value(job: &Job, col: &str) -> String {
    match col {
        "id" => job.id.clone(),
        "status" => job.status.as_str().into(),
        "title" => job.title.clone(),
        "company" => job.company.clone(),
        "location" => job.location.clone(),
        "source" => job.source.clone(),
        "url" => job.url.clone(),
        "applied_via" => job.applied_via.clone().unwrap_or_default(),
        "applied_at" => job.applied_at.clone().unwrap_or_default(),
        "posted_at" => job.posted_at.clone().unwrap_or_default(),
        "created_at" => job.created_at.clone(),
        "updated_at" => job.updated_at.clone(),
        "notes" => job.notes.clone(),
        _ => String::new(),
    }
}

pub fn to_csv(jobs: &[Job]) -> String {
    let mut out = String::new();
    out.push_str(&CSV_COLUMNS.join(","));
    out.push('\n');
    for j in jobs {
        let row: Vec<String> = CSV_COLUMNS.iter().map(|c| csv_value(j, c)).collect();
        out.push_str(&util::csv_row(&row));
        out.push('\n');
    }
    out
}

fn md_cell(s: &str) -> String {
    s.replace('|', "\\|").replace(['\n', '\r'], " ")
}

pub fn to_markdown(jobs: &[Job]) -> String {
    let mut out = String::from("# open-apply export\n\n");
    out.push_str(&format!("{} job(s), exported {}.\n\n", jobs.len(), util::now_rfc3339()));
    out.push_str("| id | status | company | title | location | via | applied |\n|---|---|---|---|---|---|---|\n");
    for j in jobs {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            j.id,
            j.status.as_str(),
            md_cell(&j.company),
            md_cell(&j.title),
            md_cell(&j.location),
            j.applied_via.as_deref().unwrap_or(""),
            j.applied_at.as_deref().map(|a| a.split('T').next().unwrap_or(a)).unwrap_or("")
        ));
    }
    out
}

#[derive(Default)]
pub struct ImportReport {
    pub imported: u32,
    pub skipped_existing: u32,
    pub events: u32,
    pub skipped: Vec<Value>,
}

impl ImportReport {
    pub fn to_value(&self, file: &str, kind: &str) -> Value {
        json!({
            "file": file,
            "kind": kind,
            "imported": self.imported,
            "skipped_existing": self.skipped_existing,
            "events_imported": self.events,
            "skipped": self.skipped,
        })
    }
}

/// Imports an export produced by `to_json`. Jobs already present (same id or canonical URL) are skipped.
pub fn import_json(conn: &Connection, text: &str) -> Result<ImportReport> {
    let export: Export = serde_json::from_str(text).map_err(|e| {
        Error::validation(format!("not an open-apply JSON export: {e}"))
            .hint("produce one with `open-apply export --format json`")
    })?;
    if export.format != FORMAT_NAME {
        return Err(Error::validation(format!("unexpected export format '{}'", export.format)));
    }
    if export.version > FORMAT_VERSION {
        return Err(Error::validation(format!("export version {} is newer than this build reads", export.version))
            .hint("upgrade open-apply"));
    }
    let mut report = ImportReport::default();
    db::in_tx(conn, |c| {
        for item in export.jobs {
            let mut job = item.job;
            if job.canonical_url.is_empty() {
                job.canonical_url = url::canonicalize(&job.url)?;
            }
            if db::find_job(c, &job.id)?.is_some() || db::find_by_canonical(c, &job.canonical_url)?.is_some() {
                report.skipped_existing += 1;
                continue;
            }
            db::insert_job_row(c, &job)?;
            for e in item.events {
                db::import_event(c, &job.id, &e)?;
                report.events += 1;
            }
            report.imported += 1;
        }
        Ok(())
    })?;
    Ok(report)
}

/// Imports jobs from a CSV with a header row. Needs `url` and `title`; everything else is optional.
/// A row with `applied_at` and no `status` becomes an applied job.
pub fn import_csv(conn: &Connection, text: &str) -> Result<ImportReport> {
    let rows = util::parse_csv(text);
    let Some((header, body)) = rows.split_first() else {
        return Err(Error::validation("the CSV file is empty"));
    };
    let header: Vec<String> = header.iter().map(|h| h.trim().to_ascii_lowercase()).collect();
    let col = |name: &str| header.iter().position(|h| h == name);
    let (Some(url_col), Some(_title_col)) = (col("url"), col("title")) else {
        return Err(Error::validation("CSV needs at least 'url' and 'title' columns")
            .hint(format!("known columns: {}", CSV_COLUMNS.join(", "))));
    };
    let get = |row: &Vec<String>, name: &str| {
        col(name).and_then(|i| row.get(i)).map(|s| s.trim().to_string()).unwrap_or_default()
    };
    let _ = url_col;

    let mut report = ImportReport::default();
    db::in_tx(conn, |c| {
        for (n, row) in body.iter().enumerate() {
            let line = n + 2;
            let url_text = get(row, "url");
            let title = get(row, "title");
            if url_text.is_empty() || title.is_empty() {
                report.skipped.push(json!({"line": line, "reason": "missing url or title"}));
                continue;
            }
            let canonical = match url::canonicalize(&url_text) {
                Ok(c) => c,
                Err(e) => {
                    report.skipped.push(json!({"line": line, "reason": e.message}));
                    continue;
                }
            };
            if db::find_by_canonical(c, &canonical)?.is_some() {
                report.skipped_existing += 1;
                continue;
            }
            let applied_at = Some(get(row, "applied_at"))
                .filter(|s| !s.is_empty())
                .and_then(|s| util::parse_timestamp(&s))
                .map(util::format_rfc3339);
            let status_text = get(row, "status");
            let status = if status_text.is_empty() {
                if applied_at.is_some() { Status::Applied } else { Status::Saved }
            } else {
                match Status::parse(&status_text) {
                    Ok(s) => s,
                    Err(e) => {
                        report.skipped.push(json!({"line": line, "reason": e.message}));
                        continue;
                    }
                }
            };
            let now = util::now_rfc3339();
            let site = url::classify(&canonical);
            let via = Some(get(row, "applied_via")).filter(|s| !s.is_empty());
            let job = Job {
                id: db::pick_id(c, &canonical)?,
                url: url_text,
                canonical_url: canonical,
                title,
                company: get(row, "company"),
                location: get(row, "location"),
                remote: None,
                source: Some(get(row, "source")).filter(|s| !s.is_empty()).unwrap_or_else(|| "import".into()),
                ats: site.ats(),
                description: String::new(),
                notes: get(row, "notes"),
                status,
                tracking_only: matches!(site, url::Site::TrackingOnly(_)),
                cv_path: None,
                cover_path: None,
                posted_at: Some(get(row, "posted_at")).filter(|s| !s.is_empty()),
                applied_at: applied_at
                    .clone()
                    .filter(|_| status != Status::Lead && status != Status::Saved && status != Status::Ready),
                applied_via: via,
                created_at: now.clone(),
                updated_at: now.clone(),
            };
            db::insert_job_row(c, &job)?;
            db::append_event(
                c,
                &db::NewEvent {
                    job_id: &job.id,
                    kind: EventType::Created,
                    status: Some(if job.applied_at.is_some() { Status::Saved } else { job.status }),
                    note: Some("imported from CSV"),
                    at: &now,
                    source: "import",
                },
            )?;
            report.events += 1;
            if let Some(at) = &job.applied_at {
                db::append_event(
                    c,
                    &db::NewEvent {
                        job_id: &job.id,
                        kind: EventType::Applied,
                        status: Some(Status::Applied),
                        note: None,
                        at,
                        source: "import",
                    },
                )?;
                report.events += 1;
                if job.status != Status::Applied {
                    db::append_event(
                        c,
                        &db::NewEvent {
                            job_id: &job.id,
                            kind: job.status.event_type(),
                            status: Some(job.status),
                            note: None,
                            at,
                            source: "import",
                        },
                    )?;
                    report.events += 1;
                }
            }
            report.imported += 1;
        }
        Ok(())
    })?;
    Ok(report)
}

pub fn looks_like_json(text: &str) -> bool {
    text.trim_start_matches('\u{feff}').trim_start().starts_with('{')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Insert, NewJob};

    fn seed(conn: &Connection) -> Job {
        let Insert::Created(mut j) = db::insert_job(
            conn,
            NewJob {
                url: "https://acme.com/jobs/1".into(),
                title: "Rust, Engineer".into(),
                company: "Acme".into(),
                location: "Berlin".into(),
                source: "manual".into(),
                ..Default::default()
            },
            "cli",
            true,
        )
        .unwrap() else {
            panic!()
        };
        db::write_applied(conn, &mut j, "2026-09-01T10:00:00Z", crate::model::Via::Ats, Some("done"), "cli").unwrap();
        *j
    }

    #[test]
    fn json_round_trip_preserves_jobs_and_events() {
        let src = db::memory();
        let job = seed(&src);
        let json = to_json(&build(db::all_jobs(&src).unwrap(), db::all_events(&src).unwrap())).unwrap();

        let dst = db::memory();
        let report = import_json(&dst, &json).unwrap();
        assert_eq!((report.imported, report.events), (1, 2));
        let back = db::get_job(&dst, &job.id).unwrap();
        assert_eq!(back.status, Status::Applied);
        assert_eq!(back.applied_at.as_deref(), Some("2026-09-01T10:00:00Z"));
        let ev = db::events_for(&dst, &job.id).unwrap();
        assert_eq!(ev.len(), 2);
        let applied = ev.iter().find(|e| e.kind == EventType::Applied).unwrap();
        assert_eq!(applied.note.as_deref(), Some("done"));
        assert_eq!(applied.at, "2026-09-01T10:00:00Z");

        // Importing again changes nothing.
        let again = import_json(&dst, &json).unwrap();
        assert_eq!((again.imported, again.skipped_existing), (0, 1));
        assert_eq!(db::events_for(&dst, &job.id).unwrap().len(), 2);
    }

    #[test]
    fn json_import_rejects_other_files() {
        let conn = db::memory();
        assert!(import_json(&conn, "{\"hello\": 1}").is_err());
        assert!(import_json(&conn, "not json").is_err());
    }

    #[test]
    fn csv_export_quotes_fields() {
        let conn = db::memory();
        seed(&conn);
        let csv = to_csv(&db::all_jobs(&conn).unwrap());
        assert!(csv.starts_with("id,status,title,company"));
        assert!(csv.contains("\"Rust, Engineer\""));
    }

    #[test]
    fn csv_round_trip_via_import() {
        let src = db::memory();
        seed(&src);
        let csv = to_csv(&db::all_jobs(&src).unwrap());
        let dst = db::memory();
        let report = import_csv(&dst, &csv).unwrap();
        assert_eq!(report.imported, 1);
        let job = &db::all_jobs(&dst).unwrap()[0];
        assert_eq!(job.title, "Rust, Engineer");
        assert_eq!(job.status, Status::Applied);
        assert_eq!(job.applied_at.as_deref(), Some("2026-09-01T10:00:00Z"));
    }

    #[test]
    fn csv_import_from_a_spreadsheet() {
        let conn = db::memory();
        let text = "Title,Company,URL,Applied_At\nEngineer,Globex,https://globex.com/j/1,2026-08-20\nBad,,,\nSaved role,Initech,https://initech.com/j/2,\n";
        let report = import_csv(&conn, text).unwrap();
        assert_eq!(report.imported, 2);
        assert_eq!(report.skipped.len(), 1);
        let jobs = db::all_jobs(&conn).unwrap();
        let globex = jobs.iter().find(|j| j.company == "Globex").unwrap();
        assert_eq!(globex.status, Status::Applied);
        let initech = jobs.iter().find(|j| j.company == "Initech").unwrap();
        assert_eq!(initech.status, Status::Saved);
        assert!(initech.applied_at.is_none());
    }

    #[test]
    fn csv_needs_url_and_title() {
        assert!(import_csv(&db::memory(), "a,b\n1,2\n").is_err());
        assert!(import_csv(&db::memory(), "").is_err());
    }

    #[test]
    fn markdown_escapes_pipes() {
        let conn = db::memory();
        let mut j = seed(&conn);
        j.title = "A | B".into();
        assert!(to_markdown(&[j]).contains("A \\| B"));
    }
}
