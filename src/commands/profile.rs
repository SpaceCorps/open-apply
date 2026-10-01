use serde_json::json;

use super::{Ctx, Out};
use crate::error::Result;

pub fn show(ctx: &Ctx) -> Result<Out> {
    ctx.home.require_initialized()?;
    let p = ctx.profile()?;
    Ok(Out::Data(json!({
        "path": ctx.home.profile_path().to_string_lossy(),
        "complete": p.missing_required().is_empty(),
        "missing_required": p.missing_required(),
        "missing_recommended": p.missing_recommended(),
        "profile": serde_json::to_value(&p)?,
    })))
}

pub fn path(ctx: &Ctx) -> Result<Out> {
    Ok(Out::Data(json!({"path": ctx.home.profile_path().to_string_lossy()})))
}

pub fn set(ctx: &Ctx, key: &str, value: &str) -> Result<Out> {
    ctx.home.require_initialized()?;
    let mut p = ctx.profile()?;
    p.set(key.trim(), value)?;
    p.save(&ctx.home)?;
    let cv_missing = key.trim() == "cv_path" && !value.trim().is_empty() && !std::path::Path::new(&p.cv_path).is_file();
    if cv_missing {
        crate::output::status(format!("warning: {} does not exist yet", p.cv_path));
    }
    Ok(Out::Data(json!({
        "set": key.trim(),
        "cleared": value.trim().is_empty(),
        "missing_required": p.missing_required(),
        "missing_recommended": p.missing_recommended(),
    })))
}
