use serde_json::json;

use super::{Ctx, Out};
use crate::cli::AppliedArgs;
use crate::db;
use crate::error::{Error, Result};
use crate::guardrails::{self, Violation};
use crate::model::{Status, Via};
use crate::url::{self, Site};
use crate::util;

/// Channel to assume when `--via` is not given.
fn infer_via(canonical: &str) -> Via {
    match url::classify(canonical) {
        Site::TrackingOnly("linkedin") => Via::Linkedin,
        Site::Greenhouse { .. } | Site::Lever { .. } | Site::Ashby { .. } => Via::Ats,
        _ => Via::Other,
    }
}

pub fn run(ctx: &Ctx, args: AppliedArgs) -> Result<Out> {
    let conn = ctx.conn()?;
    let cfg = ctx.config()?;
    let profile = ctx.profile()?;
    let at = util::resolve_at(args.at.as_deref())?;

    let result = db::in_tx(&conn, |c| {
        let mut job = db::get_job(c, &args.id)?;
        if let Some(prev) = &job.applied_at {
            return Err(Error::conflict(format!("{} was already recorded as applied on {prev}", job.id))
                .hint("use `open-apply event add` for follow-ups and responses, or `job update --applied-at` to correct the date"));
        }
        if !job.status.is_queue() {
            return Err(Error::validation(format!("{} is {}, not open for applying", job.id, job.status.as_str()))
                .hint(format!("if that is wrong, run `open-apply status {} saved` first", job.id)));
        }
        let via = match &args.via {
            Some(v) => Via::parse(v)?,
            None => infer_via(&job.canonical_url),
        };

        let violations = guardrails::check(c, &cfg, &profile, &job, &at)?;
        if !violations.is_empty() && !args.force {
            return Err(Error::guardrail(violations.iter().map(Violation::message).collect::<Vec<_>>().join("; "))
                .hint(
                    "do not mass-apply; pick a better-fitting role, or pass --force if you have decided this is right",
                )
                .detail(json!({"violations": violations.iter().map(Violation::to_value).collect::<Vec<_>>()})));
        }

        let mut note = args.note.clone().unwrap_or_default();
        if !violations.is_empty() {
            let codes: Vec<&str> = violations.iter().map(Violation::code).collect();
            if !note.is_empty() {
                note.push_str(" | ");
            }
            note.push_str(&format!("forced past: {}", codes.join(", ")));
        }
        db::write_applied(c, &mut job, &at, via, Some(&note), "cli")?;
        let in_window = db::count_applied_between(c, &guardrails::window_start(&at), &at, None)?;
        Ok((job, via, violations, in_window))
    })?;

    let (job, via, violations, in_window) = result;
    let cap = cfg.daily_application_cap;
    let mut out = json!({
        "id": job.id,
        "status": Status::Applied,
        "company": job.company,
        "title": job.title,
        "applied_at": job.applied_at,
        "via": via.as_str(),
        "applied_last_24h": in_window,
        "remaining_today": (cap > 0).then(|| cap.saturating_sub(in_window)),
    });
    if !violations.is_empty() {
        out["forced"] = json!(true);
        out["overridden"] = json!(violations.iter().map(Violation::to_value).collect::<Vec<_>>());
    }
    out["hint"] =
        json!(format!("if nothing comes back, `open-apply followups` and `open-apply stale` will surface {}", job.id));
    Ok(Out::Data(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn via_inference() {
        assert_eq!(infer_via("https://www.linkedin.com/jobs/view/3812345678"), Via::Linkedin);
        assert_eq!(infer_via("https://boards.greenhouse.io/acme/jobs/1"), Via::Ats);
        assert_eq!(infer_via("https://jobs.lever.co/acme/abc"), Via::Ats);
        assert_eq!(infer_via("https://fi.indeed.com/viewjob?jk=abc"), Via::Other);
        assert_eq!(infer_via("https://acme.com/careers/1"), Via::Other);
    }
}
