pub trait Clock {
    fn timestamp() -> i64;
    fn rfc3339() -> String;
    fn unix_millis() -> u128;
    fn unix_nanos() -> u128;
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

    fn unix_nanos() -> u128 {
        std::cfg_select! {
            miri => 0,
            _ => {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
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

pub fn now_unix_nanos() -> u128 {
    SystemClock::unix_nanos()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_clock_timestamp() {
        let ts = SystemClock::timestamp();
        // Should be a reasonable timestamp (after 2020, before 2100)
        assert!(
            ts > 1577836800,
            "timestamp should be after 2020-01-01, got {}",
            ts
        );
        assert!(ts < 4102444800, "timestamp should be before 2100-01-01");
    }

    #[test]
    fn test_system_clock_rfc3339() {
        let rfc = SystemClock::rfc3339();
        // RFC3339 format: YYYY-MM-DDTHH:MM:SS+00:00 or similar
        assert!(
            rfc.len() >= 20,
            "RFC3339 should be at least 20 chars, got {}",
            rfc
        );
        assert!(rfc.contains('T'), "RFC3339 should contain T separator");
    }

    #[test]
    fn test_system_clock_unix_millis() {
        let ms = SystemClock::unix_millis();
        // Should be a reasonable millis value (after 2020)
        assert!(
            ms > 1577836800000,
            "unix_millis should be after 2020, got {}",
            ms
        );
    }

    #[test]
    fn test_now_timestamp() {
        let ts = now_timestamp();
        assert!(ts > 1577836800);
    }

    #[test]
    fn test_now_rfc3339() {
        let rfc = now_rfc3339();
        assert!(rfc.contains('T'));
    }

    #[test]
    fn test_now_unix_millis() {
        let ms = now_unix_millis();
        assert!(ms > 1577836800000);
    }

    #[test]
    fn test_supports() {
        #[cfg(not(miri))]
        {
            assert!(supports(Capability::RealtimeClock));
            assert!(supports(Capability::IsolatedFsIo));
        }
    }

    #[test]
    fn test_capability_debug() {
        let cap = Capability::RealtimeClock;
        let s = format!("{:?}", cap);
        assert!(!s.is_empty());
    }
}
