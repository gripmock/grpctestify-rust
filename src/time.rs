use crate::polyfill::runtime;

pub fn now_timestamp() -> i64 {
    runtime::now_timestamp()
}

pub fn now_rfc3339() -> String {
    runtime::now_rfc3339()
}

pub fn now_unix_millis() -> u128 {
    runtime::now_unix_millis()
}
