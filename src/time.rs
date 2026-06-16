//! Time helpers.
//!
//! The current implementation stores Unix timestamps because Claude Code
//! statusline input uses epoch reset times. Keeping conversion here avoids
//! sprinkling clock access through rendering and persistence code.

use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
