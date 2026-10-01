use serde_json::json;

use super::{Ctx, Out};
use crate::cli::{EventAddArgs, StatusArgs};
use crate::db;
use crate::error::{Error, Result};
use crate::model::{EventType, Status};
use crate::util;

pub fn set(ctx: &Ctx, args: StatusArgs) -> Result<Out> {
    let conn = ctx.conn()?;
    let new = Status::parse(&args.status)?;
    if new == Status::Applied {
        return Err(Error::validation("'applied' is recorded with `open-apply applied <id>`")
            .hint("that command enforces the daily cap and company cooldown and keeps the application date"));
    }
    let at = util::resolve_at(args.at.as_deref())?;
    let mut job = db::get_job(&conn, &args.id)?;
    let from = job.status;
    if from == new {
        return Err(Error::conflict(format!("{} is already {}", job.id, new.as_str()))
            .hint("use `open-apply event add --type note` to add a remark"));
    }
    db::change_status(&conn, &mut job, new, new.event_type(), args.note.as_deref(), &at, "cli")?;
    Ok(Out::Data(json!({"id": job.id, "from": from, "to": new, "at": at})))
}

pub fn add_event(ctx: &Ctx, args: EventAddArgs) -> Result<Out> {
    let kind = EventType::parse(&args.kind)?;
    if !EventType::USER_ADDABLE.contains(&kind) {
        return Err(Error::validation(format!("'{}' cannot be added by hand", kind.as_str()))
            .hint(format!("valid types: {}", EventType::USER_ADDABLE.map(EventType::as_str).join(", "))));
    }
    let at = util::resolve_at(args.at.as_deref())?;
    let conn = ctx.conn()?;
    let mut job = db::get_job(&conn, &args.id)?;
    let from = job.status;
    let moved = db::record_event(&conn, &mut job, kind, args.note.as_deref(), &at, "cli")?;
    Ok(Out::Data(json!({
        "id": job.id,
        "event": kind,
        "at": at,
        "status_before": from,
        "status": job.status,
        "status_changed": moved.is_some(),
    })))
}
