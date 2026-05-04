use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};

pub(crate) fn current_unix_ms() -> std::result::Result<u64, SystemTimeError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
}
