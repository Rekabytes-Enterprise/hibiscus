pub(crate) fn compact_tokens(value: u64) -> String {
    for (unit, suffix, next) in [
        (1_000_000_000_000u64, "T", None),
        (1_000_000_000, "B", Some("T")),
        (1_000_000, "M", Some("B")),
        (1_000, "K", Some("M")),
    ] {
        if value >= unit {
            let tenths = (u128::from(value) * 10 + u128::from(unit) / 2) / u128::from(unit);
            if tenths >= 10_000 {
                if let Some(next) = next {
                    return format!("1{next}");
                }
            }
            if tenths % 10 == 0 {
                return format!("{}{suffix}", tenths / 10);
            }
            return format!("{}.{}{suffix}", tenths / 10, tenths % 10);
        }
    }
    value.to_string()
}
