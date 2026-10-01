use serde_json::json;

use super::{Ctx, Out};
use crate::cli::StaleArgs;
use crate::db::{self, JobFilter};
use crate::error::Result;
use crate::model::{EventType, Status};
use crate::util::{self, DAY};

pub fn run(ctx: &Ctx, args: StaleArgs) -> Result<Out> {
    let conn = ctx.conn()?;
    let now_secs = util::now_secs();
    let now = util::format_rfc3339(now_secs);
    let cutoff = util::format_rfc3339(now_secs - i64::from(args.days) * DAY);

    let applied = db::list_jobs(&conn, &JobFilter { statuses: vec![Status::Applied], ..Default::default() })?;
    let mut stale: Vec<_> =
        applied.into_iter().filter(|j| j.applied_at.as_deref().is_some_and(|a| a <= cutoff.as_str())).collect();
    stale.sort_by(|a, b| a.applied_at.cmp(&b.applied_at));

    let mut listing = Vec::new();
    let mut marked = 0;
    for mut job in stale {
        let applied_at = job.applied_at.clone().unwrap_or_default();
        let days = util::days_between(&applied_at, &now).unwrap_or(0.0).floor() as i64;
        listing.push(json!({
            "id": job.id,
            "company": job.company,
            "title": job.title,
            "applied_at": applied_at,
            "days_waiting": days,
            "marked": args.mark,
        }));
        if args.mark {
            let note = format!("no response in {} days", args.days);
            db::change_status(&conn, &mut job, Status::Ghosted, EventType::Status, Some(&note), &now, "stale")?;
            marked += 1;
        }
    }
    Ok(Out::Data(json!({
        "days": args.days,
        "count": listing.len(),
        "marked": marked,
        "jobs": listing,
        "hint": if args.mark || listing.is_empty() { serde_json::Value::Null } else { json!("rerun with --mark to set these to ghosted; a late reply still moves them forward") },
    })))
}
