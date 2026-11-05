//! String conversion only; the safe API owns environment validation and replacement.
use super::{ffi_call, pg_str_to_string, unsafe_mut, PgMsg, PgStr};
use crate::api::{self, PgError};

/// Adds/replaces one new_session environment entry. Set the request id first.
/// Reserved capability variables, invalid strings and budgets fail without mutation.
#[no_mangle]
pub extern "C" fn pg_msg_set_session_env(msg: *mut PgMsg, name: PgStr, value: PgStr) -> bool {
    ffi_call(false, || {
        if name.size < std::mem::size_of::<PgStr>() as u32
            || value.size < std::mem::size_of::<PgStr>() as u32
            || name.len > 255
            || value.len > 32 * 1024
        {
            return Err(PgError::InvalidArg);
        }
        let name = pg_str_to_string(name)?;
        let value = pg_str_to_string(value)?;
        let handle = unsafe_mut(msg)?;
        let mut message = handle.to_message()?;
        api::set_session_env(&mut message, &name, &value)?;
        *handle = PgMsg::from_message(&message)?;
        Ok(true)
    })
}
