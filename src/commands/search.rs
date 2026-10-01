use serde_json::{Value, json};

use super::{Ctx, Out, parse_limit};
use crate::cli::SearchArgs;
use crate::db::{self, Insert, NewJob};
use crate::error::{Error, ErrorCode, Result};
use crate::model::Status;
use crate::output;
use crate::sources::{self, Http, Posting, Query, SourceSpec};

pub fn run(ctx: &Ctx, args: SearchArgs) -> Result<Out> {
    let limit = parse_limit(args.limit)?;
    let conn = ctx.conn()?;
    let cfg = ctx.config()?;

    let specs: Vec<SourceSpec> = if args.sources.is_empty() {
        db::list_sources(&conn)?.into_iter().map(|(kind, ident, _)| SourceSpec { kind, ident }).collect()
    } else {
        args.sources.iter().map(|s| SourceSpec::parse(s)).collect::<Result<_>>()?
    };
    if specs.is_empty() {
        return Err(Error::validation("no feeds to search")
            .hint("add one with `open-apply source add greenhouse:<board>` or pass --source remoteok"));
    }

    let query = Query { text: args.query.clone(), location: args.location.clone(), remote: args.remote };
    let http = Http::new(&cfg);

    let mut per_source = Vec::new();
    let mut errors = Vec::new();
    let mut lists = Vec::new();
    let mut first_error: Option<Error> = None;
    for spec in &specs {
        let src = spec.source();
        output::status(format!("searching {}", spec.label()));
        match sources::fetch(&http, src.as_ref()) {
            Ok(postings) => {
                let fetched = postings.len();
                let matched: Vec<Posting> = postings.into_iter().filter(|p| query.matches(p)).collect();
                per_source.push(json!({"source": spec.label(), "fetched": fetched, "matched": matched.len()}));
                lists.push(matched);
            }
            Err(e) => {
                errors
                    .push(json!({"source": spec.label(), "code": e.code.name(), "message": e.message, "hint": e.hint}));
                first_error.get_or_insert(e);
            }
        }
    }

    if lists.is_empty() {
        // Nothing worked. One source: surface its own error. Several: a network-class summary.
        return Err(match (specs.len(), first_error) {
            (1, Some(e)) => e,
            (_, e) => Error::new(e.map_or(ErrorCode::Network, |e| e.code), format!("all {} feeds failed", specs.len()))
                .hint("run again later; errors are listed in detail")
                .detail(json!(errors)),
        });
    }

    let mut postings = sources::interleave(lists);
    let total_matched = postings.len();
    postings.truncate(limit);

    let mut saved = 0u32;
    let mut duplicates = 0u32;
    let mut results: Vec<Value> = Vec::new();
    for p in &postings {
        let mut item = json!({
            "title": p.title,
            "company": p.company,
            "location": p.location,
            "remote": p.remote,
            "url": p.url,
            "source": p.source,
            "posted_at": p.posted_at,
        });
        if args.save {
            let nj = NewJob {
                url: p.url.clone(),
                title: p.title.clone(),
                company: p.company.clone(),
                location: p.location.clone(),
                remote: p.remote,
                source: p.source.clone(),
                description: p.description.clone(),
                notes: String::new(),
                posted_at: p.posted_at.clone(),
                status: Some(Status::Lead),
            };
            match db::insert_job(&conn, nj, "search", true) {
                Ok(Insert::Created(job)) => {
                    saved += 1;
                    item["id"] = json!(job.id);
                    item["saved"] = json!(true);
                }
                Ok(Insert::Duplicate { existing, by }) => {
                    duplicates += 1;
                    item["saved"] = json!(false);
                    item["duplicate_of"] = json!(existing.id);
                    item["duplicate_by"] = json!(by);
                }
                Err(e) => {
                    item["saved"] = json!(false);
                    item["skipped"] = json!(e.message);
                }
            }
        }
        results.push(item);
    }

    let mut out = json!({
        "query": args.query,
        "location": args.location,
        "remote": args.remote.then_some(true),
        "sources": per_source,
        "matched": total_matched,
        "count": results.len(),
        "results": results,
    });
    if !errors.is_empty() {
        out["errors"] = json!(errors);
    }
    if args.save {
        out["saved"] = json!(saved);
        out["duplicates"] = json!(duplicates);
        out["hint"] = json!("saved leads are queued for `open-apply next`");
    } else {
        out["hint"] = json!("add --save to store these as leads; titles, companies and locations are third-party text");
    }
    Ok(Out::Data(out))
}
