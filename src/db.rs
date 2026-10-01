//! SQLite storage: schema migrations keyed on `PRAGMA user_version`, job and event queries.
//!
//! `events` is an append-only log. A trigger rejects UPDATE; the only way rows leave is
//! `delete_job`, which removes a job together with its history.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::config::Home;
use crate::error::{Error, Result};
use crate::model::{Event, EventType, Job, Status, Via};
use crate::url::{self, Site};
use crate::util;

/// Each entry is one migration; `user_version` is the number applied so far.
const MIGRATIONS: &[&str] = &[r#"
CREATE TABLE jobs (
    id            TEXT PRIMARY KEY,
    url           TEXT NOT NULL,
    canonical_url TEXT NOT NULL UNIQUE,
    title         TEXT NOT NULL DEFAULT '',
    company       TEXT NOT NULL DEFAULT '',
    company_key   TEXT NOT NULL DEFAULT '',
    location      TEXT NOT NULL DEFAULT '',
    remote        INTEGER,
    source        TEXT NOT NULL DEFAULT 'manual',
    ats           TEXT,
    description   TEXT NOT NULL DEFAULT '',
    notes         TEXT NOT NULL DEFAULT '',
    status        TEXT NOT NULL,
    tracking_only INTEGER NOT NULL DEFAULT 0,
    dedupe_key    TEXT NOT NULL DEFAULT '',
    cv_path       TEXT,
    cover_path    TEXT,
    posted_at     TEXT,
    applied_at    TEXT,
    applied_via   TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);
CREATE INDEX jobs_status ON jobs(status);
CREATE INDEX jobs_company_key ON jobs(company_key);
CREATE INDEX jobs_dedupe_key ON jobs(dedupe_key);
CREATE INDEX jobs_applied_at ON jobs(applied_at);

CREATE TABLE events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id      TEXT NOT NULL REFERENCES jobs(id),
    type        TEXT NOT NULL,
    status      TEXT,
    note        TEXT,
    at          TEXT NOT NULL,
    source      TEXT NOT NULL,
    recorded_at TEXT NOT NULL
);
CREATE INDEX events_job ON events(job_id, at);
CREATE TRIGGER events_append_only BEFORE UPDATE ON events
BEGIN
    SELECT RAISE(ABORT, 'events are append-only');
END;

CREATE TABLE sources (
    kind     TEXT NOT NULL,
    ident    TEXT NOT NULL,
    added_at TEXT NOT NULL,
    PRIMARY KEY (kind, ident)
);
"#];

pub fn latest_schema_version() -> i64 {
    MIGRATIONS.len() as i64
}

fn configure(conn: &Connection) -> Result<()> {
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(())
}

pub fn migrate(conn: &Connection) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if current > latest_schema_version() {
        return Err(Error::validation(format!(
            "database schema v{current} is newer than this build understands (v{})",
            latest_schema_version()
        ))
        .hint("upgrade open-apply"));
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        in_tx(conn, |c| {
            c.execute_batch(sql)?;
            c.pragma_update(None, "user_version", i as i64 + 1)?;
            Ok(())
        })?;
    }
    Ok(())
}

/// Creates the database file if needed and brings it to the latest schema.
pub fn init(home: &Home) -> Result<Connection> {
    let conn = Connection::open(home.db_path())?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

/// Opens an initialized home.
pub fn open(home: &Home) -> Result<Connection> {
    home.require_initialized()?;
    let conn = Connection::open(home.db_path())?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn schema_version(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

/// Runs `f` in an immediate transaction so check-then-write sequences (guardrails) cannot race.
pub fn in_tx<T>(conn: &Connection, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    match f(conn) {
        Ok(v) => {
            conn.execute_batch("COMMIT")?;
            Ok(v)
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------------------------

const JOB_COLS: &str = "id, url, canonical_url, title, company, location, remote, source, ats, description, notes, \
     status, tracking_only, cv_path, cover_path, posted_at, applied_at, applied_via, created_at, updated_at";

fn job_from_row(r: &Row) -> rusqlite::Result<Job> {
    let status: String = r.get(11)?;
    let status = Status::parse(&status)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(11, rusqlite::types::Type::Text, Box::new(e)))?;
    Ok(Job {
        id: r.get(0)?,
        url: r.get(1)?,
        canonical_url: r.get(2)?,
        title: r.get(3)?,
        company: r.get(4)?,
        location: r.get(5)?,
        remote: r.get::<_, Option<i64>>(6)?.map(|v| v != 0),
        source: r.get(7)?,
        ats: r.get(8)?,
        description: r.get(9)?,
        notes: r.get(10)?,
        status,
        tracking_only: r.get::<_, i64>(12)? != 0,
        cv_path: r.get(13)?,
        cover_path: r.get(14)?,
        posted_at: r.get(15)?,
        applied_at: r.get(16)?,
        applied_via: r.get(17)?,
        created_at: r.get(18)?,
        updated_at: r.get(19)?,
    })
}

/// Accepts `oa_1a2b3c4d` or the bare 8 hex characters.
pub fn normalize_id(id: &str) -> String {
    let t = id.trim().to_ascii_lowercase();
    if t.starts_with("oa_") { t } else { format!("oa_{t}") }
}

pub fn find_job(conn: &Connection, id: &str) -> Result<Option<Job>> {
    let sql = format!("SELECT {JOB_COLS} FROM jobs WHERE id = ?1");
    Ok(conn.query_row(&sql, [normalize_id(id)], job_from_row).optional()?)
}

pub fn get_job(conn: &Connection, id: &str) -> Result<Job> {
    find_job(conn, id)?.ok_or_else(|| {
        Error::not_found(format!("no job with id '{}'", id.trim())).hint("run `open-apply job list` to see ids")
    })
}

pub fn find_by_canonical(conn: &Connection, canonical: &str) -> Result<Option<Job>> {
    let sql = format!("SELECT {JOB_COLS} FROM jobs WHERE canonical_url = ?1");
    Ok(conn.query_row(&sql, [canonical], job_from_row).optional()?)
}

fn find_by_dedupe_key(conn: &Connection, key: &str) -> Result<Option<Job>> {
    let sql = format!("SELECT {JOB_COLS} FROM jobs WHERE dedupe_key = ?1 ORDER BY created_at LIMIT 1");
    Ok(conn.query_row(&sql, [key], job_from_row).optional()?)
}

#[derive(Default, Clone, Debug)]
pub struct JobFilter {
    pub statuses: Vec<Status>,
    pub company: Option<String>,
    pub since: Option<String>,
    pub limit: Option<usize>,
}

pub fn list_jobs(conn: &Connection, f: &JobFilter) -> Result<Vec<Job>> {
    let mut sql = format!("SELECT {JOB_COLS} FROM jobs WHERE 1=1");
    let mut args: Vec<rusqlite::types::Value> = Vec::new();
    if !f.statuses.is_empty() {
        let marks: Vec<String> = f.statuses.iter().map(|_| "?".to_string()).collect();
        sql.push_str(&format!(" AND status IN ({})", marks.join(",")));
        args.extend(f.statuses.iter().map(|s| rusqlite::types::Value::Text(s.as_str().into())));
    }
    if let Some(c) = &f.company {
        let escaped = c.to_lowercase().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
        sql.push_str(" AND lower(company) LIKE ? ESCAPE '\\'");
        args.push(rusqlite::types::Value::Text(format!("%{escaped}%")));
    }
    if let Some(s) = &f.since {
        sql.push_str(" AND created_at >= ?");
        args.push(rusqlite::types::Value::Text(s.clone()));
    }
    sql.push_str(" ORDER BY created_at DESC, id");
    if let Some(l) = f.limit {
        sql.push_str(&format!(" LIMIT {l}"));
    }
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(args), job_from_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn all_jobs(conn: &Connection) -> Result<Vec<Job>> {
    list_jobs(conn, &JobFilter::default())
}

/// Inserts a complete job row (used by `insert_job` and by import).
pub fn insert_job_row(conn: &Connection, j: &Job) -> Result<()> {
    conn.execute(
        "INSERT INTO jobs (id, url, canonical_url, title, company, company_key, location, remote, source, ats, \
         description, notes, status, tracking_only, dedupe_key, cv_path, cover_path, posted_at, applied_at, \
         applied_via, created_at, updated_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22)",
        params![
            j.id,
            j.url,
            j.canonical_url,
            j.title,
            j.company,
            util::company_key(&j.company),
            j.location,
            j.remote.map(i64::from),
            j.source,
            j.ats,
            j.description,
            j.notes,
            j.status.as_str(),
            i64::from(j.tracking_only),
            util::dedupe_key(&j.company, &j.title, &j.location),
            j.cv_path,
            j.cover_path,
            j.posted_at,
            j.applied_at,
            j.applied_via,
            j.created_at,
            j.updated_at,
        ],
    )?;
    Ok(())
}

/// Writes every mutable column of `j` back and bumps `updated_at`. Keys are recomputed.
pub fn save_job(conn: &Connection, j: &Job) -> Result<()> {
    conn.execute(
        "UPDATE jobs SET url=?2, title=?3, company=?4, company_key=?5, location=?6, remote=?7, source=?8, \
         description=?9, notes=?10, status=?11, dedupe_key=?12, cv_path=?13, cover_path=?14, posted_at=?15, \
         applied_at=?16, applied_via=?17, updated_at=?18 WHERE id=?1",
        params![
            j.id,
            j.url,
            j.title,
            j.company,
            util::company_key(&j.company),
            j.location,
            j.remote.map(i64::from),
            j.source,
            j.description,
            j.notes,
            j.status.as_str(),
            util::dedupe_key(&j.company, &j.title, &j.location),
            j.cv_path,
            j.cover_path,
            j.posted_at,
            j.applied_at,
            j.applied_via,
            util::now_rfc3339(),
        ],
    )?;
    Ok(())
}

pub fn delete_job(conn: &Connection, id: &str) -> Result<usize> {
    in_tx(conn, |c| {
        let n = c.execute("DELETE FROM events WHERE job_id = ?1", [id])?;
        c.execute("DELETE FROM jobs WHERE id = ?1", [id])?;
        Ok(n)
    })
}

#[derive(Clone, Debug, Default)]
pub struct NewJob {
    pub url: String,
    pub title: String,
    pub company: String,
    pub location: String,
    pub remote: Option<bool>,
    pub source: String,
    pub description: String,
    pub notes: String,
    pub posted_at: Option<String>,
    pub status: Option<Status>,
}

pub enum Insert {
    Created(Box<Job>),
    /// Already stored: `by` is `url` or `listing` (same company, title and location).
    Duplicate {
        existing: Box<Job>,
        by: &'static str,
    },
}

/// Picks `oa_` + 8 hex characters from the SHA-256 of the canonical URL. On the (unlikely)
/// collision with a different URL, the next 8 characters of the digest are used.
pub fn pick_id(conn: &Connection, canonical: &str) -> Result<String> {
    let digest = util::hex(&util::sha256(canonical.as_bytes()));
    for i in 0..8 {
        let candidate = format!("oa_{}", &digest[i * 8..i * 8 + 8]);
        let taken: Option<String> =
            conn.query_row("SELECT canonical_url FROM jobs WHERE id = ?1", [&candidate], |r| r.get(0)).optional()?;
        match taken {
            None => return Ok(candidate),
            Some(c) if c == canonical => return Ok(candidate),
            Some(_) => continue,
        }
    }
    Err(Error::internal("could not derive a unique job id"))
}

pub fn insert_job(conn: &Connection, nj: NewJob, event_source: &str, check_listing: bool) -> Result<Insert> {
    let canonical = url::canonicalize(&nj.url)?;
    in_tx(conn, |c| {
        if let Some(existing) = find_by_canonical(c, &canonical)? {
            return Ok(Insert::Duplicate { existing: Box::new(existing), by: "url" });
        }
        if check_listing && !nj.company.trim().is_empty() && !nj.title.trim().is_empty() {
            let key = util::dedupe_key(&nj.company, &nj.title, &nj.location);
            if let Some(existing) = find_by_dedupe_key(c, &key)? {
                return Ok(Insert::Duplicate { existing: Box::new(existing), by: "listing" });
            }
        }
        let site = url::classify(&canonical);
        let now = util::now_rfc3339();
        let job = Job {
            id: pick_id(c, &canonical)?,
            url: nj.url.trim().to_string(),
            canonical_url: canonical,
            title: nj.title.trim().to_string(),
            company: nj.company.trim().to_string(),
            location: nj.location.trim().to_string(),
            remote: nj.remote,
            source: if nj.source.is_empty() { "manual".into() } else { nj.source },
            ats: site.ats(),
            description: nj.description,
            notes: nj.notes,
            status: nj.status.unwrap_or(Status::Saved),
            tracking_only: matches!(site, Site::TrackingOnly(_)),
            cv_path: None,
            cover_path: None,
            posted_at: nj.posted_at,
            applied_at: None,
            applied_via: None,
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        insert_job_row(c, &job)?;
        append_event(
            c,
            &NewEvent {
                job_id: &job.id,
                kind: EventType::Created,
                status: Some(job.status),
                note: None,
                at: &now,
                source: event_source,
            },
        )?;
        Ok(Insert::Created(Box::new(job)))
    })
}

// ---------------------------------------------------------------------------------------------
// Events and status changes
// ---------------------------------------------------------------------------------------------

pub struct NewEvent<'a> {
    pub job_id: &'a str,
    pub kind: EventType,
    pub status: Option<Status>,
    pub note: Option<&'a str>,
    pub at: &'a str,
    pub source: &'a str,
}

pub fn append_event(conn: &Connection, e: &NewEvent) -> Result<()> {
    conn.execute(
        "INSERT INTO events (job_id, type, status, note, at, source, recorded_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            e.job_id,
            e.kind.as_str(),
            e.status.map(Status::as_str),
            e.note.filter(|n| !n.trim().is_empty()),
            e.at,
            e.source,
            util::now_rfc3339()
        ],
    )?;
    Ok(())
}

fn event_from_row(r: &Row) -> rusqlite::Result<Event> {
    let bad =
        |i: usize, e: Error| rusqlite::Error::FromSqlConversionFailure(i, rusqlite::types::Type::Text, Box::new(e));
    let kind: String = r.get(2)?;
    let status: Option<String> = r.get(3)?;
    Ok(Event {
        id: r.get(0)?,
        job_id: r.get(1)?,
        kind: EventType::parse(&kind).map_err(|e| bad(2, e))?,
        status: status.map(|s| Status::parse(&s)).transpose().map_err(|e| bad(3, e))?,
        note: r.get(4)?,
        at: r.get(5)?,
        source: r.get(6)?,
        recorded_at: r.get(7)?,
    })
}

const EVENT_COLS: &str = "id, job_id, type, status, note, at, source, recorded_at";

pub fn events_for(conn: &Connection, job_id: &str) -> Result<Vec<Event>> {
    let sql = format!("SELECT {EVENT_COLS} FROM events WHERE job_id = ?1 ORDER BY (type != 'created'), at, id");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([job_id], event_from_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn all_events(conn: &Connection) -> Result<Vec<Event>> {
    let sql = format!("SELECT {EVENT_COLS} FROM events ORDER BY job_id, (type != 'created'), at, id");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], event_from_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Restores an event from an export, keeping its original timestamps.
pub fn import_event(conn: &Connection, job_id: &str, e: &crate::export::EventRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO events (job_id, type, status, note, at, source, recorded_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            job_id,
            e.kind.as_str(),
            e.status.map(Status::as_str),
            e.note,
            e.at,
            e.source,
            e.recorded_at.clone().unwrap_or_else(|| e.at.clone())
        ],
    )?;
    Ok(())
}

/// Moves a job to `new` and appends the matching event, atomically.
pub fn change_status(
    conn: &Connection,
    job: &mut Job,
    new: Status,
    kind: EventType,
    note: Option<&str>,
    at: &str,
    source: &str,
) -> Result<()> {
    in_tx(conn, |c| {
        job.status = new;
        save_job(c, job)?;
        append_event(c, &NewEvent { job_id: &job.id, kind, status: Some(new), note, at, source })
    })
}

/// Appends an event; if `advance` says the status should move, moves it in the same transaction.
/// Returns the new status when it changed.
pub fn record_event(
    conn: &Connection,
    job: &mut Job,
    kind: EventType,
    note: Option<&str>,
    at: &str,
    source: &str,
) -> Result<Option<Status>> {
    in_tx(conn, |c| {
        let moved = crate::model::advance(job.status, kind);
        if let Some(s) = moved {
            job.status = s;
            save_job(c, job)?;
        }
        append_event(c, &NewEvent { job_id: &job.id, kind, status: moved, note, at, source })?;
        Ok(moved)
    })
}

/// Marks the job applied. Guardrail checks happen before this is called, in the same transaction.
pub fn write_applied(
    conn: &Connection,
    job: &mut Job,
    at: &str,
    via: Via,
    note: Option<&str>,
    source: &str,
) -> Result<()> {
    job.status = Status::Applied;
    job.applied_at = Some(at.to_string());
    job.applied_via = Some(via.as_str().to_string());
    save_job(conn, job)?;
    append_event(
        conn,
        &NewEvent { job_id: &job.id, kind: EventType::Applied, status: Some(Status::Applied), note, at, source },
    )
}

// ---------------------------------------------------------------------------------------------
// Guardrail queries
// ---------------------------------------------------------------------------------------------

/// Applications with `applied_at` in `(after, up_to]`.
pub fn count_applied_between(conn: &Connection, after: &str, up_to: &str, exclude_id: Option<&str>) -> Result<u32> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM jobs WHERE applied_at > ?1 AND applied_at <= ?2 AND id != ?3",
        params![after, up_to, exclude_id.unwrap_or("")],
        |r| r.get(0),
    )?;
    Ok(n as u32)
}

/// Other applications to the same company (by normalized name): `(id, title, applied_at)`.
pub fn company_applications(
    conn: &Connection,
    company_key: &str,
    exclude_id: &str,
) -> Result<Vec<(String, String, String)>> {
    if company_key.is_empty() {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT id, title, applied_at FROM jobs WHERE company_key = ?1 AND applied_at IS NOT NULL AND id != ?2 \
         ORDER BY applied_at DESC",
    )?;
    let rows = stmt.query_map(params![company_key, exclude_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

// ---------------------------------------------------------------------------------------------
// Sources
// ---------------------------------------------------------------------------------------------

pub fn add_source(conn: &Connection, kind: &str, ident: &str) -> Result<bool> {
    let n = conn.execute(
        "INSERT OR IGNORE INTO sources (kind, ident, added_at) VALUES (?1, ?2, ?3)",
        params![kind, ident, util::now_rfc3339()],
    )?;
    Ok(n > 0)
}

pub fn remove_source(conn: &Connection, kind: &str, ident: &str) -> Result<bool> {
    Ok(conn.execute("DELETE FROM sources WHERE kind = ?1 AND ident = ?2", params![kind, ident])? > 0)
}

pub fn list_sources(conn: &Connection) -> Result<Vec<(String, String, String)>> {
    let mut stmt = conn.prepare("SELECT kind, ident, added_at FROM sources ORDER BY kind, ident")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

#[cfg(test)]
pub fn memory() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    migrate(&conn).unwrap();
    conn
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new(url: &str, company: &str, title: &str) -> NewJob {
        NewJob {
            url: url.into(),
            company: company.into(),
            title: title.into(),
            location: "Berlin".into(),
            ..Default::default()
        }
    }

    #[test]
    fn migrations_set_user_version_and_are_idempotent() {
        let conn = memory();
        assert_eq!(schema_version(&conn).unwrap(), latest_schema_version());
        migrate(&conn).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), 1);
    }

    #[test]
    fn newer_schema_is_refused() {
        let conn = memory();
        conn.pragma_update(None, "user_version", 99).unwrap();
        assert!(migrate(&conn).is_err());
    }

    #[test]
    fn id_is_stable_and_shaped() {
        let conn = memory();
        let a = pick_id(&conn, "https://example.com/jobs/1").unwrap();
        assert_eq!(a, pick_id(&conn, "https://example.com/jobs/1").unwrap());
        assert!(a.starts_with("oa_") && a.len() == 11, "{a}");
        assert_ne!(a, pick_id(&conn, "https://example.com/jobs/2").unwrap());
    }

    #[test]
    fn insert_dedupes_on_url_and_listing() {
        let conn = memory();
        let Insert::Created(first) =
            insert_job(&conn, new("https://acme.com/jobs/1?utm_source=x", "Acme", "Engineer"), "cli", true).unwrap()
        else {
            panic!("expected created")
        };
        assert_eq!(first.status, Status::Saved);
        match insert_job(&conn, new("https://www.acme.com/jobs/1/", "Acme", "Engineer"), "cli", true).unwrap() {
            Insert::Duplicate { existing, by } => assert_eq!((existing.id.as_str(), by), (first.id.as_str(), "url")),
            _ => panic!("expected url duplicate"),
        }
        match insert_job(&conn, new("https://linkedin.example/jobs/9", "ACME Inc.", "engineer"), "cli", true).unwrap() {
            Insert::Duplicate { by, .. } => assert_eq!(by, "listing"),
            _ => panic!("expected listing duplicate"),
        }
        assert!(matches!(
            insert_job(&conn, new("https://other.example/jobs/9", "ACME Inc.", "engineer"), "cli", false).unwrap(),
            Insert::Created(_)
        ));
        // Every insert logs a `created` event.
        assert_eq!(events_for(&conn, &first.id).unwrap().len(), 1);
    }

    #[test]
    fn events_are_append_only() {
        let conn = memory();
        let Insert::Created(j) =
            insert_job(&conn, new("https://acme.com/jobs/1", "Acme", "Engineer"), "cli", true).unwrap()
        else {
            panic!()
        };
        assert!(conn.execute("UPDATE events SET note = 'tampered' WHERE job_id = ?1", [&j.id]).is_err());
    }

    #[test]
    fn status_change_appends_event() {
        let conn = memory();
        let Insert::Created(mut j) =
            insert_job(&conn, new("https://acme.com/jobs/1", "Acme", "Engineer"), "cli", true).unwrap()
        else {
            panic!()
        };
        change_status(&conn, &mut j, Status::Ready, EventType::Status, Some("prepared"), &util::now_rfc3339(), "cli")
            .unwrap();
        let reread = get_job(&conn, &j.id).unwrap();
        assert_eq!(reread.status, Status::Ready);
        let ev = events_for(&conn, &j.id).unwrap();
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[1].status, Some(Status::Ready));
        assert_eq!(ev[1].note.as_deref(), Some("prepared"));
    }

    #[test]
    fn delete_removes_history() {
        let conn = memory();
        let Insert::Created(j) =
            insert_job(&conn, new("https://acme.com/jobs/1", "Acme", "Engineer"), "cli", true).unwrap()
        else {
            panic!()
        };
        assert_eq!(delete_job(&conn, &j.id).unwrap(), 1);
        assert!(find_job(&conn, &j.id).unwrap().is_none());
        assert!(all_events(&conn).unwrap().is_empty());
    }

    #[test]
    fn id_lookup_accepts_bare_hex() {
        let conn = memory();
        let Insert::Created(j) =
            insert_job(&conn, new("https://acme.com/jobs/1", "Acme", "Engineer"), "cli", true).unwrap()
        else {
            panic!()
        };
        assert_eq!(get_job(&conn, &j.id[3..]).unwrap().id, j.id);
        assert!(get_job(&conn, "oa_00000000").is_err());
    }

    #[test]
    fn filters_and_counts() {
        let conn = memory();
        for (i, c) in ["Acme", "Globex"].iter().enumerate() {
            let Insert::Created(mut j) =
                insert_job(&conn, new(&format!("https://x.example/{i}"), c, "Engineer"), "cli", false).unwrap()
            else {
                panic!()
            };
            write_applied(&conn, &mut j, &format!("2026-09-0{}T10:00:00Z", i + 1), Via::Ats, None, "cli").unwrap();
        }
        let f = JobFilter { company: Some("glob".into()), ..Default::default() };
        assert_eq!(list_jobs(&conn, &f).unwrap().len(), 1);
        let f = JobFilter { statuses: vec![Status::Applied], limit: Some(1), ..Default::default() };
        assert_eq!(list_jobs(&conn, &f).unwrap().len(), 1);
        assert_eq!(count_applied_between(&conn, "2026-08-31T00:00:00Z", "2026-09-01T23:00:00Z", None).unwrap(), 1);
        assert_eq!(count_applied_between(&conn, "2026-08-31T00:00:00Z", "2026-09-05T00:00:00Z", None).unwrap(), 2);
        assert_eq!(company_applications(&conn, "acme", "").unwrap().len(), 1);
        assert_eq!(company_applications(&conn, "", "").unwrap().len(), 0);
    }

    #[test]
    fn sources_crud() {
        let conn = memory();
        assert!(add_source(&conn, "greenhouse", "acme").unwrap());
        assert!(!add_source(&conn, "greenhouse", "acme").unwrap());
        assert_eq!(list_sources(&conn).unwrap().len(), 1);
        assert!(remove_source(&conn, "greenhouse", "acme").unwrap());
        assert!(!remove_source(&conn, "greenhouse", "acme").unwrap());
    }
}
