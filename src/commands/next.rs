use serde_json::{Value, json};

use super::{Ctx, Out, materials_value, parse_limit, resolve_answers, resolved_profile, wrapped_description};
use crate::cli::NextArgs;
use crate::db::{self, JobFilter};
use crate::error::{Error, Result};
use crate::guardrails::{self, Violation};
use crate::model::{Job, Status};
use crate::{url, util};

const DESCRIPTION_CHARS: usize = 2_000;

fn queue_rank(s: Status) -> u8 {
    match s {
        Status::Ready => 0,
        Status::Saved => 1,
        _ => 2,
    }
}

fn via_hint(job: &Job) -> &'static str {
    match url::classify(&job.canonical_url) {
        url::Site::TrackingOnly("linkedin") => "linkedin",
        url::Site::Greenhouse { .. } | url::Site::Lever { .. } | url::Site::Ashby { .. } => "ats",
        _ => "other",
    }
}

pub fn run(ctx: &Ctx, args: NextArgs) -> Result<Out> {
    let count = parse_limit(args.count)?;
    let conn = ctx.conn()?;
    let cfg = ctx.config()?;
    let profile = ctx.profile()?;
    let now = util::now_rfc3339();

    let applied_24h = db::count_applied_between(&conn, &guardrails::window_start(&now), &now, None)?;
    let cap = cfg.daily_application_cap;
    let remaining: Option<u32> = (cap > 0).then(|| cap.saturating_sub(applied_24h));
    let guardrail_state = json!({
        "daily_application_cap": cap,
        "applied_last_24h": applied_24h,
        "remaining_today": remaining,
        "company_cooldown_days": cfg.company_cooldown_days,
        "require_materials": cfg.require_materials,
        "max_followups": cfg.max_followups,
    });

    if remaining == Some(0) && !args.force {
        return Err(Error::guardrail(format!("daily cap reached: {applied_24h} application(s) in the last 24 hours, cap is {cap}"))
            .hint("stop for today; quality beats volume. To change the limit edit daily_application_cap in config.yaml, or pass --force on purpose")
            .detail(json!({"guardrails": guardrail_state})));
    }

    let mut candidates = db::list_jobs(
        &conn,
        &JobFilter { statuses: vec![Status::Lead, Status::Saved, Status::Ready], ..Default::default() },
    )?;
    candidates.sort_by(|a, b| {
        queue_rank(a.status)
            .cmp(&queue_rank(b.status))
            .then_with(|| a.created_at.cmp(&b.created_at))
            .then_with(|| a.id.cmp(&b.id))
    });

    let allowed = match remaining {
        Some(r) if !args.force => count.min(r as usize),
        _ => count,
    };

    let mut items: Vec<Value> = Vec::new();
    let mut blocked: Vec<Value> = Vec::new();
    for job in &candidates {
        if items.len() >= allowed {
            break;
        }
        // Only cooldown and materials are per job; the cap was handled above.
        let mut per_job: Vec<Violation> = guardrails::check(&conn, &cfg, &profile, job, &now)?;
        per_job.retain(|v| !matches!(v, Violation::DailyCap { .. }));
        if !per_job.is_empty() && !args.force {
            blocked.push(json!({
                "id": job.id,
                "title": job.title,
                "company": job.company,
                "reasons": per_job.iter().map(Violation::to_value).collect::<Vec<_>>(),
            }));
            continue;
        }
        let workspace = ctx.home.workspace(&job.id);
        let mut item = json!({
            "id": job.id,
            "status": job.status,
            "title": job.title,
            "company": job.company,
            "location": job.location,
            "remote": job.remote,
            "url": job.url,
            "source": job.source,
            "tracking_only": job.tracking_only.then_some(true),
            "apply_via": via_hint(job),
            "description": wrapped_description(job, DESCRIPTION_CHARS),
            "workspace": workspace.is_dir().then(|| workspace.to_string_lossy().into_owned()),
            "materials": materials_value(job, &profile),
            "profile": resolved_profile(&profile, job),
            "answers": resolve_answers(&profile, job),
        });
        if !per_job.is_empty() {
            item["forced_past"] = json!(per_job.iter().map(Violation::to_value).collect::<Vec<_>>());
        }
        items.push(item);
    }

    if items.is_empty() && !blocked.is_empty() {
        return Err(Error::guardrail(format!("{} candidate(s) are blocked by guardrails", blocked.len()))
            .hint("they are listed in detail; pick another role, wait out the cooldown, or pass --force on purpose")
            .detail(json!({"guardrails": guardrail_state, "blocked": blocked})));
    }

    let mut out = json!({
        "guardrails": guardrail_state,
        "count": items.len(),
        "jobs": items,
    });
    if !blocked.is_empty() {
        out["blocked"] = json!(blocked);
    }
    if candidates.is_empty() {
        out["hint"] = json!("the queue is empty: run `open-apply search --save` or `open-apply job add <url>`");
    } else if items.len() < count && remaining.is_some_and(|r| (r as usize) < count) && !args.force {
        out["hint"] = json!("the list is shortened to what is left of today's cap");
    }
    Ok(Out::Data(out))
}
