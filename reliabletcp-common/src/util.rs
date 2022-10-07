
pub fn get_current_timestamp() -> u64 {
    chrono::Utc::now().timestamp_millis() as u64
}

pub fn bool_to_u8(value: bool) -> u8 {
    if value {
        1
    } else {
        0
    }
}

pub fn u8_to_bool(value: u8) -> bool {
    value != 0
}