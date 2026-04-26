use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn unix_timestamp_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock drifted before unix epoch")
        .as_secs()
}
