use std::path::Path;

use serde_json::json;

use super::{Ctx, Out};
use crate::cli::ExportArgs;
use crate::config::{absolute, write_atomic};
use crate::error::{Error, Result};
use crate::{db, export};

pub fn export(ctx: &Ctx, args: ExportArgs) -> Result<Out> {
    let conn = ctx.conn()?;
    let format = args.format.trim().to_ascii_lowercase();
    if !matches!(format.as_str(), "json" | "csv" | "md") {
        return Err(Error::validation(format!("unknown export format '{}'", args.format)).hint("use json, csv or md"));
    }
    let mut jobs = db::all_jobs(&conn)?;
    jobs.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.id.cmp(&b.id)));
    let events = db::all_events(&conn)?;
    let event_count = events.len();
    let job_count = jobs.len();

    let text = match format.as_str() {
        "json" => export::to_json(&export::build(jobs, events))?,
        "csv" => export::to_csv(&jobs),
        _ => export::to_markdown(&jobs),
    };
    match args.out {
        Some(path) => {
            let path = absolute(&path);
            write_atomic(&path, &text)?;
            Ok(Out::Data(
                json!({"out": path.to_string_lossy(), "format": format, "jobs": job_count, "events": event_count}),
            ))
        }
        None => Ok(Out::Raw(text)),
    }
}

pub fn import(ctx: &Ctx, file: &Path) -> Result<Out> {
    let conn = ctx.conn()?;
    let path = absolute(file);
    let text = std::fs::read_to_string(&path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => Error::not_found(format!("{} does not exist", path.display())),
        _ => Error::validation(format!("cannot read {} as text: {e}", path.display())),
    })?;
    let (kind, report) = if export::looks_like_json(&text) {
        ("json", export::import_json(&conn, &text)?)
    } else {
        ("csv", export::import_csv(&conn, &text)?)
    };
    Ok(Out::Data(report.to_value(&path.to_string_lossy(), kind)))
}
