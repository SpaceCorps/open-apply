use super::{Ctx, Out};
use crate::cli::StatsArgs;
use crate::error::Result;
use crate::{db, stats, util};

pub fn run(ctx: &Ctx, args: StatsArgs) -> Result<Out> {
    let conn = ctx.conn()?;
    let since = args.since.as_deref().map(util::parse_since).transpose()?;
    let jobs = db::all_jobs(&conn)?;
    let events = db::all_events(&conn)?;
    Ok(Out::Data(stats::compute(&jobs, &events, since.as_deref())))
}
