use serde_json::json;

use super::{Ctx, Out};
use crate::cli::FollowupsArgs;
use crate::db::{self, JobFilter};
use crate::error::Result;
use crate::model::{EventType, Status};
use crate::util;

/// Statuses where an employer might still answer a nudge.
const FOLLOWUP_STATUSES: [Status; 5] =
    [Status::Applied, Status::Acknowledged, Status::Screening, Status::Assessment, Status::Interview];

pub fn run(ctx: &Ctx, args: FollowupsArgs) -> Result<Out> {
    let conn = ctx.conn()?;
    let cfg = ctx.config()?;
    let now = util::now_rfc3339();

    let jobs = db::list_jobs(&conn, &JobFilter { statuses: FOLLOWUP_STATUSES.to_vec(), ..Default::default() })?;
    let mut due = Vec::new();
    for job in jobs {
        let Some(applied_at) = job.applied_at.clone() else { continue };
        let events = db::events_for(&conn, &job.id)?;
        let last_activity = events
            .iter()
            .filter(|e| !matches!(e.kind, EventType::Note | EventType::Created))
            .map(|e| e.at.as_str())
            .max()
            .unwrap_or(applied_at.as_str())
            .to_string();
        let last_response = events
            .iter()
            .filter(|e| e.kind.is_response())
            .map(|e| e.at.as_str())
            .max()
            .unwrap_or(applied_at.as_str())
            .to_string();
        let sent =
            events.iter().filter(|e| e.kind == EventType::FollowUp && e.at.as_str() >= last_response.as_str()).count()
                as u32;
        let idle = util::days_between(&last_activity, &now).unwrap_or(0.0);
        if idle >= f64::from(args.days) && sent < cfg.max_followups {
            due.push((
                idle,
                json!({
                    "id": job.id,
                    "company": job.company,
                    "title": job.title,
                    "status": job.status,
                    "applied_at": applied_at,
                    "days_since_activity": idle.floor() as i64,
                    "follow_ups_sent": sent,
                }),
            ));
        }
    }
    due.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let jobs: Vec<_> = due.into_iter().map(|(_, v)| v).collect();
    Ok(Out::Data(json!({
        "days": args.days,
        "max_followups": cfg.max_followups,
        "count": jobs.len(),
        "jobs": jobs,
        "hint": if jobs.is_empty() { serde_json::Value::Null } else { json!("send one short, polite note, then record it: open-apply event add <id> --type follow_up") },
    })))
}
