use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::{Ctx, Out, job_summary, materials_value, parse_limit, wrapped_description};
use crate::cli::{JobAddArgs, JobListArgs, JobUpdateArgs};
use crate::config::absolute;
use crate::db::{self, Insert, NewJob};
use crate::error::{Error, Result};
use crate::model::{EventType, Status, Via};
use crate::sources::{self, Http, Posting};
use crate::url::{self, Site};
use crate::{output, util};

pub fn add(ctx: &Ctx, args: JobAddArgs) -> Result<Out> {
    let conn = ctx.conn()?;
    let canonical = url::canonicalize(&args.url)?;
    let site = url::classify(&canonical);
    let given_title = args.title.clone().filter(|s| !s.trim().is_empty());
    let given_company = args.company.clone().filter(|s| !s.trim().is_empty());

    let mut warnings: Vec<String> = Vec::new();
    let mut fetched: Option<Posting> = None;
    let mut fetch_attempted = false;

    match &site {
        Site::TrackingOnly(name) => {
            warnings.push(format!(
                "{name} URLs are tracked only: open-apply never fetches or scrapes this site, so title and company come from you"
            ));
        }
        _ if args.no_fetch => {}
        _ => {
            fetch_attempted = true;
            let cfg = ctx.config()?;
            let http = Http::new(&cfg);
            let page_url = args.url.trim();
            let result = match &site {
                Site::Other => sources::fetch_page_posting(&http, page_url),
                _ => sources::fetch_ats_posting(&http, &site),
            };
            match result {
                Ok(p) => {
                    if p.is_none() && matches!(site, Site::Other) {
                        warnings.push("no schema.org JobPosting found on the page; storing what was given".into());
                    }
                    fetched = p;
                }
                Err(e) => {
                    let known_ats = !matches!(site, Site::Other);
                    // A known ATS without a title and company is useless to store blind.
                    if known_ats && (given_title.is_none() || given_company.is_none()) && !e.message.is_empty() {
                        return Err(Error::new(e.code, format!("could not read the posting: {}", e.message))
                            .hint("retry later, or add it offline with --no-fetch --title <t> --company <c>"));
                    }
                    warnings.push(format!("could not read the page ({}); storing what was given", e.message));
                }
            }
        }
    }

    let title = given_title.or_else(|| fetched.as_ref().map(|p| p.title.clone()).filter(|t| !t.is_empty()));
    let Some(title) = title else {
        let why = if matches!(site, Site::TrackingOnly(_)) {
            "tracking-only sites are never fetched"
        } else {
            "no title was found on the page"
        };
        return Err(Error::validation(format!("cannot add a job without a title ({why})"))
            .hint("pass --title <title> and --company <company>"));
    };
    let company = given_company
        .or_else(|| fetched.as_ref().map(|p| p.company.clone()).filter(|c| !c.is_empty()))
        .unwrap_or_default();
    if company.is_empty() {
        warnings.push("no company name: pass --company so cooldown and email triage can match this job".into());
    }
    let location = args.location.clone().or_else(|| fetched.as_ref().map(|p| p.location.clone())).unwrap_or_default();
    let source = args.source.clone().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| match (&site, &fetched) {
        (Site::Other, _) | (_, None) => "manual".into(),
        (_, Some(p)) => p.source.clone(),
    });

    let nj = NewJob {
        url: args.url.trim().to_string(),
        title,
        company,
        location,
        remote: fetched.as_ref().and_then(|p| p.remote),
        source,
        description: fetched.as_ref().map(|p| p.description.clone()).unwrap_or_default(),
        notes: args.notes.clone().unwrap_or_default(),
        posted_at: fetched.as_ref().and_then(|p| p.posted_at.clone()),
        status: Some(Status::Saved),
    };
    match db::insert_job(&conn, nj, "cli", !args.allow_duplicate)? {
        Insert::Created(job) => {
            for w in &warnings {
                output::status(format!("warning: {w}"));
            }
            let mut out = json!({"job": job_summary(&job), "fetched": fetched.is_some(), "description_chars": job.description.chars().count()});
            if fetch_attempted && fetched.is_none() {
                out["fetched"] = json!(false);
            }
            if !warnings.is_empty() {
                out["warnings"] = json!(warnings);
            }
            out["next"] = json!(format!("open-apply prepare {}", job.id));
            Ok(Out::Data(out))
        }
        Insert::Duplicate { existing, by } => {
            let how = if by == "url" { "the same URL" } else { "the same company, title and location" };
            Err(Error::conflict(format!("already stored as {} ({how})", existing.id))
                .hint(format!("use `open-apply job show {}`; pass --allow-duplicate to add it anyway", existing.id))
                .detail(json!({"existing_id": existing.id, "matched_by": by, "status": existing.status})))
        }
    }
}

pub fn list(ctx: &Ctx, args: JobListArgs) -> Result<Out> {
    let conn = ctx.conn()?;
    let statuses = match &args.status {
        Some(s) => s.split(',').filter(|p| !p.trim().is_empty()).map(Status::parse).collect::<Result<Vec<_>>>()?,
        None => Vec::new(),
    };
    let filter = db::JobFilter {
        statuses,
        company: args.company.clone().filter(|c| !c.trim().is_empty()),
        since: args.since.as_deref().map(util::parse_since).transpose()?,
        limit: Some(parse_limit(args.limit)?),
    };
    let jobs = db::list_jobs(&conn, &filter)?;
    Ok(Out::Data(json!({"count": jobs.len(), "jobs": jobs.iter().map(job_summary).collect::<Vec<_>>()})))
}

pub fn show(ctx: &Ctx, id: &str) -> Result<Out> {
    let conn = ctx.conn()?;
    let profile = ctx.profile()?;
    let job = db::get_job(&conn, id)?;
    let events: Vec<Value> = db::events_for(&conn, &job.id)?
        .iter()
        .map(|e| json!({"type": e.kind, "status": e.status, "note": e.note, "at": e.at, "source": e.source}))
        .collect();
    let workspace = ctx.home.workspace(&job.id);
    let mut v = job_summary(&job);
    v["canonical_url"] = json!(job.canonical_url);
    v["remote"] = json!(job.remote);
    v["ats"] = json!(job.ats);
    v["posted_at"] = json!(job.posted_at);
    if !job.notes.is_empty() {
        v["notes"] = json!(job.notes);
    }
    v["materials"] = materials_value(&job, &profile);
    if workspace.is_dir() {
        v["workspace"] = json!(workspace.to_string_lossy());
    }
    v["description"] = wrapped_description(&job, 20_000).map_or(Value::Null, Value::String);
    v["events"] = json!(events);
    Ok(Out::Data(v))
}

pub fn update(ctx: &Ctx, args: JobUpdateArgs) -> Result<Out> {
    let conn = ctx.conn()?;
    let mut job = db::get_job(&conn, &args.id)?;
    let mut changed: Vec<&str> = Vec::new();
    let mut note: Option<String> = None;

    if let Some(t) = args.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        job.title = t.into();
        changed.push("title");
    }
    if let Some(c) = args.company.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        job.company = c.into();
        changed.push("company");
    }
    if let Some(l) = args.location.as_deref() {
        job.location = l.trim().into();
        changed.push("location");
    }
    if let Some(n) = args.notes.as_deref() {
        job.notes = n.into();
        changed.push("notes");
    }
    if let Some(s) = args.source.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        job.source = s.into();
        changed.push("source");
    }
    if args.applied_at.is_some() || args.via.is_some() {
        let Some(prev_at) = job.applied_at.clone() else {
            return Err(Error::validation("this job has not been applied to yet")
                .hint("use `open-apply applied <id>` to record an application"));
        };
        let mut parts = Vec::new();
        if let Some(a) = &args.applied_at {
            job.applied_at = Some(util::resolve_at(Some(a))?);
            parts.push(format!("applied_at {prev_at} -> {}", job.applied_at.as_deref().unwrap_or("")));
            changed.push("applied_at");
        }
        if let Some(v) = &args.via {
            let via = Via::parse(v)?;
            parts.push(format!("via {} -> {}", job.applied_via.as_deref().unwrap_or("unknown"), via.as_str()));
            job.applied_via = Some(via.as_str().into());
            changed.push("via");
        }
        note = Some(format!("corrected: {}", parts.join(", ")));
    }
    if changed.is_empty() {
        return Err(Error::validation("nothing to update")
            .hint("pass at least one of --title --company --location --notes --source --applied-at --via"));
    }
    db::in_tx(&conn, |c| {
        db::save_job(c, &job)?;
        if let Some(n) = &note {
            let at = util::now_rfc3339();
            db::append_event(
                c,
                &db::NewEvent {
                    job_id: &job.id,
                    kind: EventType::Note,
                    status: None,
                    note: Some(n),
                    at: &at,
                    source: "cli",
                },
            )?;
        }
        Ok(())
    })?;
    Ok(Out::Data(json!({"updated": changed, "job": job_summary(&job)})))
}

pub fn rm(ctx: &Ctx, id: &str, yes: bool) -> Result<Out> {
    let conn = ctx.conn()?;
    let job = db::get_job(&conn, id)?;
    if !yes {
        return Err(Error::validation(format!("refusing to delete {} without --yes", job.id))
            .hint(format!("this removes the job and its history; run `open-apply job rm {} --yes`", job.id)));
    }
    let events = db::delete_job(&conn, &job.id)?;
    let workspace = ctx.home.workspace(&job.id);
    let mut out = json!({"deleted": job.id, "events_deleted": events});
    if workspace.is_dir() {
        out["workspace_left_in_place"] = json!(workspace.to_string_lossy());
    }
    Ok(Out::Data(out))
}

fn existing_file(label: &str, p: &Path) -> Result<String> {
    let abs = absolute(p);
    if !abs.is_file() {
        return Err(Error::validation(format!("{label} file not found: {}", abs.display()))
            .hint("pass the path of an existing file"));
    }
    Ok(abs.to_string_lossy().into_owned())
}

pub fn attach(ctx: &Ctx, id: &str, cv: Option<PathBuf>, cover: Option<PathBuf>) -> Result<Out> {
    if cv.is_none() && cover.is_none() {
        return Err(Error::usage("nothing to attach").hint("pass --cv <path> and/or --cover <path>"));
    }
    let conn = ctx.conn()?;
    let mut job = db::get_job(&conn, id)?;
    if let Some(p) = &cv {
        job.cv_path = Some(existing_file("CV", p)?);
    }
    if let Some(p) = &cover {
        job.cover_path = Some(existing_file("cover letter", p)?);
    }
    db::save_job(&conn, &job)?;
    Ok(Out::Data(json!({"id": job.id, "cv": job.cv_path, "cover_letter": job.cover_path})))
}
