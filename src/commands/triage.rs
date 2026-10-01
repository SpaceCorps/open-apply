use std::io::Read;

use serde_json::{Value, json};

use super::{Ctx, Out};
use crate::cli::TriageArgs;
use crate::db;
use crate::error::{Error, Result};
use crate::model::Job;
use crate::triage::{self, Email, Match};
use crate::{output, util};

const MAX_STDIN: u64 = 2 * 1024 * 1024;

fn read_stdin() -> Result<String> {
    let mut buf = String::new();
    std::io::stdin()
        .take(MAX_STDIN)
        .read_to_string(&mut buf)
        .map_err(|e| Error::validation(format!("could not read stdin as text: {e}")))?;
    Ok(buf)
}

pub fn run(ctx: &Ctx, args: TriageArgs) -> Result<Out> {
    if args.from.is_none() && args.subject.is_none() && !args.stdin {
        return Err(Error::usage("nothing to triage").hint("pass --from, --subject and/or --stdin (body on stdin)"));
    }
    let email = Email {
        from: args.from.clone().unwrap_or_default(),
        subject: args.subject.clone().unwrap_or_default(),
        body: if args.stdin { read_stdin()? } else { String::new() },
    };
    let at = util::resolve_at(args.at.as_deref())?;
    let conn = ctx.conn()?;

    let class = triage::classify(&email);

    let candidates: Vec<Job> = match &args.job {
        Some(id) => vec![db::get_job(&conn, id)?],
        None => db::all_jobs(&conn)?
            .into_iter()
            .filter(|j| j.applied_at.is_some() || j.status.is_active_application())
            .collect(),
    };
    let matches: Vec<Match> = if args.job.is_some() {
        vec![Match { job_id: candidates[0].id.clone(), score: 100, reasons: vec!["chosen with --job"] }]
    } else {
        triage::match_jobs(&email, &candidates)
    };
    let chosen = triage::pick_unambiguous(&matches).cloned();
    let ambiguous = chosen.is_none() && matches.len() > 1;

    let match_values: Vec<Value> = matches
        .iter()
        .take(5)
        .map(|m| {
            let job = candidates.iter().find(|j| j.id == m.job_id);
            json!({
                "id": m.job_id,
                "company": job.map(|j| j.company.as_str()),
                "title": job.map(|j| j.title.as_str()),
                "status": job.map(|j| j.status),
                "score": m.score,
                "reasons": m.reasons,
            })
        })
        .collect();

    // The sender and subject are untrusted; they are echoed only inside delimiters.
    let echoed = format!("From: {}\nSubject: {}\n\n{}", email.from, email.subject, triage::excerpt(&email, 400));

    let mut out = json!({
        "email": output::wrap_email(&echoed),
        "classification": {
            "class": class.class.as_str(),
            "confidence": class.confidence,
            "score": class.score,
            "evidence": class.evidence,
        },
        "matches": match_values,
        "ambiguous": ambiguous,
        "applied": false,
    });

    if !args.apply {
        out["hint"] = json!(match (&chosen, class.class.event_type()) {
            (Some(m), Some(_)) =>
                format!("dry run; record it with the same command plus --apply (matched {})", m.job_id),
            (None, Some(_)) => "dry run; no single application matched, pass --job <id> with --apply".to_string(),
            _ => "dry run; this email would not change anything".to_string(),
        });
        return Ok(Out::Data(out));
    }

    let Some(event_type) = class.class.event_type() else {
        out["reason"] =
            json!(format!("classified as {}: not an employer response, nothing recorded", class.class.as_str()));
        return Ok(Out::Data(out));
    };
    if class.confidence == "low" && args.job.is_none() {
        out["reason"] =
            json!("confidence is low: nothing recorded; if it is right, record it with `open-apply event add`");
        return Ok(Out::Data(out));
    }
    let detail = json!({"classification": out["classification"], "matches": out["matches"]});
    let Some(chosen) = chosen else {
        return Err(if ambiguous {
            Error::conflict("the email matches several applications equally well")
                .hint("pick one and rerun with --job <id>")
                .detail(detail)
        } else {
            Error::not_found("no application matches this email")
                .hint("rerun with --job <id>, or add the job first with `open-apply job add`")
                .detail(detail)
        });
    };

    let mut job = candidates
        .into_iter()
        .find(|j| j.id == chosen.job_id)
        .ok_or_else(|| Error::internal("matched job vanished"))?;
    let before = job.status;
    let (_, _, domain) = triage::sender_parts(&email.from);
    let note = triage::describe(class.class, class.confidence, &domain);
    let moved = db::record_event(&conn, &mut job, event_type, Some(&note), &at, "triage")?;
    out["applied"] = json!(true);
    out["recorded"] = json!({
        "job_id": job.id,
        "event": event_type,
        "status_before": before,
        "status": job.status,
        "status_changed": moved.is_some(),
        "at": at,
    });
    Ok(Out::Data(out))
}
