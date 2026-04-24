pub trait Clock {
    fn timestamp() -> i64;
    fn rfc3339() -> String;
    fn unix_millis() -> u128;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn timestamp() -> i64 {
        std::cfg_select! {
            miri => 0,
            _ => chrono::Utc::now().timestamp(),
        }
    }

    fn rfc3339() -> String {
        std::cfg_select! {
            miri => "1970-01-01T00:00:00+00:00".to_string(),
            _ => chrono::Utc::now().to_rfc3339(),
        }
    }

    fn unix_millis() -> u128 {
        std::cfg_select! {
            miri => 0,
            _ => {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            },
        }
    }
}

pub fn now_timestamp() -> i64 {
    SystemClock::timestamp()
}

pub fn now_rfc3339() -> String {
    SystemClock::rfc3339()
}

pub fn now_unix_millis() -> u128 {
    SystemClock::unix_millis()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    ProcessSpawn,
    RealtimeClock,
    IsolatedFsIo,
}

pub const fn supports(_capability: Capability) -> bool {
    std::cfg_select! {
        miri => false,
        _ => true,
    }
}
