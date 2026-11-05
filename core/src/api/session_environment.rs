//! Per-session launch overrides; validation is shared by host and C message builders.
use super::{Message, PgError, PgResult};
use std::collections::BTreeMap;

/// Validates portable names, reserved terminal variables and bounded UTF-8 data.
/// Raw wire decoding leaves this check to dispatch so errors preserve the request id.
pub fn validate_session_environment(env: &BTreeMap<String, String>) -> PgResult<()> {
    if env.len() > 128 {
        return Err(PgError::InvalidArg);
    }
    let mut total = 0usize;
    for (name, value) in env {
        let mut bytes = name.bytes();
        if name.len() > 255
            || !bytes
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
            || !bytes.all(|c| c.is_ascii_alphanumeric() || c == b'_')
            || matches!(name.as_str(), "TERM" | "COLORTERM" | "PI_PROGRAM_STATUS")
            || value.len() > 32 * 1024
            || value.contains('\0')
        {
            return Err(PgError::InvalidArg);
        }
        total = total
            .checked_add(name.len() + value.len() + 2)
            .ok_or(PgError::InvalidArg)?;
        if total > 64 * 1024 {
            return Err(PgError::InvalidArg);
        }
    }
    Ok(())
}

/// Adds or replaces one new_session override atomically. Empty values stay empty.
pub fn set_session_env(message: &mut Message, name: &str, value: &str) -> PgResult<()> {
    let Message::NewSession { env, .. } = message else {
        return Err(PgError::InvalidArg);
    };
    let mut next = env.clone().unwrap_or_default();
    next.insert(name.to_owned(), value.to_owned());
    validate_session_environment(&next)?;
    *env = Some(next);
    Ok(())
}
