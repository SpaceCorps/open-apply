use serde_json::json;

use super::{Ctx, Out};
use crate::db;
use crate::error::{Error, Result};
use crate::sources::SourceSpec;

pub fn add(ctx: &Ctx, spec: &str) -> Result<Out> {
    let spec = SourceSpec::parse(spec)?;
    let conn = ctx.conn()?;
    let added = db::add_source(&conn, &spec.kind, &spec.ident)?;
    Ok(Out::Data(json!({"source": spec.label(), "added": added, "already_watched": !added})))
}

pub fn list(ctx: &Ctx) -> Result<Out> {
    let conn = ctx.conn()?;
    let sources: Vec<_> = db::list_sources(&conn)?
        .into_iter()
        .map(|(kind, ident, added_at)| {
            let label = if ident.is_empty() { kind } else { format!("{kind}:{ident}") };
            json!({"source": label, "added_at": added_at})
        })
        .collect();
    Ok(Out::Data(json!({"count": sources.len(), "sources": sources})))
}

pub fn remove(ctx: &Ctx, spec: &str) -> Result<Out> {
    let spec = SourceSpec::parse(spec)?;
    let conn = ctx.conn()?;
    if !db::remove_source(&conn, &spec.kind, &spec.ident)? {
        return Err(Error::not_found(format!("'{}' is not a watched source", spec.label()))
            .hint("run `open-apply source list`"));
    }
    Ok(Out::Data(json!({"source": spec.label(), "removed": true})))
}
