//! Funnel statistics. Pure: jobs and events in, numbers out.
//!
//! "Response" means an employer answered in any way: acknowledgement, screen, interview,
//! assessment, offer or rejection. Response time is measured from `applied_at` to the first
//! such event on or after it.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::model::{Event, EventType, Job, Status};
use crate::util::{self, median, round1, round3};

struct Acc {
    applied: u32,
    responded: u32,
}

fn rate(responded: u32, applied: u32) -> Option<f64> {
    (applied > 0).then(|| round3(f64::from(responded) / f64::from(applied)))
}

fn breakdown(map: &BTreeMap<String, Acc>) -> Value {
    Value::Array(
        map.iter()
            .map(|(k, a)| json!({"key": k, "applied": a.applied, "responded": a.responded, "response_rate": rate(a.responded, a.applied)}))
            .collect(),
    )
}

/// The part of a source label before `:` (`greenhouse:acme` -> `greenhouse`).
pub fn source_kind(source: &str) -> &str {
    source.split(':').next().unwrap_or(source)
}

/// `since` keeps jobs whose `applied_at` (or, for jobs never applied to, `created_at`) is on or after it.
pub fn compute(jobs: &[Job], events: &[Event], since: Option<&str>) -> Value {
    let kept: Vec<&Job> =
        jobs.iter().filter(|j| since.is_none_or(|s| j.applied_at.as_deref().unwrap_or(&j.created_at) >= s)).collect();

    let mut funnel: BTreeMap<&str, u32> = Status::ALL.iter().map(|s| (s.as_str(), 0)).collect();
    for j in &kept {
        *funnel.entry(j.status.as_str()).or_default() += 1;
    }
    // Keep the pipeline order rather than alphabetical.
    let funnel_ordered: Value =
        Value::Object(Status::ALL.iter().map(|s| (s.as_str().to_string(), json!(funnel[s.as_str()]))).collect());

    let mut by_job: BTreeMap<&str, Vec<&Event>> = BTreeMap::new();
    for e in events {
        by_job.entry(e.job_id.as_str()).or_default().push(e);
    }

    let mut applied_total = 0u32;
    let mut responded = 0u32;
    let mut days: Vec<f64> = Vec::new();
    let mut by_source: BTreeMap<String, Acc> = BTreeMap::new();
    let mut by_via: BTreeMap<String, Acc> = BTreeMap::new();
    let mut reached: BTreeMap<&str, u32> =
        ["acknowledged", "screening", "assessment", "interview", "offer"].iter().map(|k| (*k, 0)).collect();

    for j in &kept {
        let Some(applied_at) = j.applied_at.as_deref() else { continue };
        applied_total += 1;
        let evs = by_job.get(j.id.as_str()).map(Vec::as_slice).unwrap_or(&[]);
        let first_response =
            evs.iter().filter(|e| e.kind.is_response() && e.at.as_str() >= applied_at).map(|e| e.at.as_str()).min();
        let did_respond = first_response.is_some();
        if let Some(at) = first_response {
            responded += 1;
            if let Some(d) = util::days_between(applied_at, at) {
                days.push(d);
            }
        }
        for (ty, key) in [
            (EventType::Ack, "acknowledged"),
            (EventType::Screen, "screening"),
            (EventType::Assessment, "assessment"),
            (EventType::Interview, "interview"),
            (EventType::Offer, "offer"),
        ] {
            if evs.iter().any(|e| e.kind == ty && e.at.as_str() >= applied_at) {
                *reached.entry(key).or_default() += 1;
            }
        }
        let src = by_source.entry(source_kind(&j.source).to_string()).or_insert(Acc { applied: 0, responded: 0 });
        src.applied += 1;
        src.responded += u32::from(did_respond);
        let via = by_via
            .entry(j.applied_via.clone().unwrap_or_else(|| "unknown".into()))
            .or_insert(Acc { applied: 0, responded: 0 });
        via.applied += 1;
        via.responded += u32::from(did_respond);
    }

    json!({
        "since": since,
        "jobs": kept.len(),
        "funnel": funnel_ordered,
        "applied": applied_total,
        "responded": responded,
        "response_rate": rate(responded, applied_total),
        "median_days_to_first_response": median(&mut days).map(round1),
        "ever_reached": reached,
        "by_source": breakdown(&by_source),
        "by_via": breakdown(&by_via),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(id: &str, source: &str, status: Status, applied_at: Option<&str>, via: Option<&str>) -> Job {
        Job {
            id: id.into(),
            url: format!("https://x.example/{id}"),
            canonical_url: format!("https://x.example/{id}"),
            title: "T".into(),
            company: "C".into(),
            location: String::new(),
            remote: None,
            source: source.into(),
            ats: None,
            description: String::new(),
            notes: String::new(),
            status,
            tracking_only: false,
            cv_path: None,
            cover_path: None,
            posted_at: None,
            applied_at: applied_at.map(Into::into),
            applied_via: via.map(Into::into),
            created_at: "2026-08-01T00:00:00Z".into(),
            updated_at: "2026-08-01T00:00:00Z".into(),
        }
    }

    fn ev(job_id: &str, kind: EventType, at: &str) -> Event {
        Event {
            id: 0,
            job_id: job_id.into(),
            kind,
            status: None,
            note: None,
            at: at.into(),
            source: "cli".into(),
            recorded_at: at.into(),
        }
    }

    #[test]
    fn empty_database() {
        let v = compute(&[], &[], None);
        assert_eq!(v["applied"], 0);
        assert!(v["response_rate"].is_null());
        assert!(v["median_days_to_first_response"].is_null());
        assert_eq!(v["funnel"]["applied"], 0);
    }

    #[test]
    fn rates_medians_and_breakdowns() {
        let jobs = vec![
            job("a", "greenhouse:acme", Status::Interview, Some("2026-09-01T00:00:00Z"), Some("ats")),
            job("b", "greenhouse:globex", Status::Rejected, Some("2026-09-01T00:00:00Z"), Some("ats")),
            job("c", "remoteok", Status::Ghosted, Some("2026-09-01T00:00:00Z"), Some("linkedin")),
            job("d", "remoteok", Status::Lead, None, None),
        ];
        let events = vec![
            ev("a", EventType::Applied, "2026-09-01T00:00:00Z"),
            ev("a", EventType::Ack, "2026-09-02T00:00:00Z"),
            ev("a", EventType::Interview, "2026-09-10T00:00:00Z"),
            ev("b", EventType::Rejection, "2026-09-05T00:00:00Z"),
            ev("c", EventType::FollowUp, "2026-09-08T00:00:00Z"),
            ev("c", EventType::Note, "2026-09-09T00:00:00Z"),
        ];
        let v = compute(&jobs, &events, None);
        assert_eq!(v["applied"], 3);
        assert_eq!(v["responded"], 2);
        assert_eq!(v["response_rate"], 0.667);
        // first responses: a after 1 day, b after 4 days -> median 2.5
        assert_eq!(v["median_days_to_first_response"], 2.5);
        assert_eq!(v["funnel"]["lead"], 1);
        assert_eq!(v["funnel"]["interview"], 1);
        assert_eq!(v["funnel"]["ghosted"], 1);
        assert_eq!(v["ever_reached"]["interview"], 1);
        assert_eq!(v["ever_reached"]["acknowledged"], 1);
        let by_source = v["by_source"].as_array().unwrap();
        let greenhouse = by_source.iter().find(|s| s["key"] == "greenhouse").unwrap();
        assert_eq!((greenhouse["applied"].as_u64(), greenhouse["responded"].as_u64()), (Some(2), Some(2)));
        let remote = by_source.iter().find(|s| s["key"] == "remoteok").unwrap();
        assert_eq!((remote["applied"].as_u64(), remote["responded"].as_u64()), (Some(1), Some(0)));
        let by_via = v["by_via"].as_array().unwrap();
        let linkedin = by_via.iter().find(|s| s["key"] == "linkedin").unwrap();
        assert_eq!(linkedin["response_rate"], 0.0);
    }

    #[test]
    fn responses_before_applying_do_not_count() {
        let jobs = vec![job("a", "manual", Status::Applied, Some("2026-09-10T00:00:00Z"), Some("email"))];
        let events = vec![ev("a", EventType::Screen, "2026-09-05T00:00:00Z")];
        let v = compute(&jobs, &events, None);
        assert_eq!(v["responded"], 0);
    }

    #[test]
    fn since_filters_on_applied_or_created() {
        let jobs = vec![
            job("old", "manual", Status::Applied, Some("2026-01-01T00:00:00Z"), Some("ats")),
            job("new", "manual", Status::Applied, Some("2026-09-01T00:00:00Z"), Some("ats")),
            job("lead", "manual", Status::Lead, None, None),
        ];
        let v = compute(&jobs, &[], Some("2026-08-15T00:00:00Z"));
        assert_eq!(v["jobs"], 1);
        assert_eq!(v["applied"], 1);
        assert_eq!(v["funnel"]["lead"], 0);
    }
}
