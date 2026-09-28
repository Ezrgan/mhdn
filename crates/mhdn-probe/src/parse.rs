use std::ops::Range;

/// Parse a guest address. Accepts `0x00100000` or `00100000` (always hexadecimal).
pub fn parse_hex_u32(input: &str) -> std::result::Result<u32, String> {
    let s = input.trim();
    let s = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    if s.is_empty() {
        return Err("empty address".to_string());
    }
    u32::from_str_radix(s, 16).map_err(|e| format!("invalid hex address '{input}': {e}"))
}

pub fn parse_hex_u64(input: &str) -> std::result::Result<u64, String> {
    let s = input.trim();
    let s = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    if s.is_empty() {
        return Err("empty value".to_string());
    }
    u64::from_str_radix(s, 16).map_err(|e| format!("invalid hex value '{input}': {e}"))
}

/// Inclusive-exclusive range `START-END`, both hexadecimal (`0x08000000-0x09000000`).
pub fn parse_range(input: &str) -> std::result::Result<Range<u32>, String> {
    let (start, end) = input
        .split_once('-')
        .ok_or_else(|| format!("range '{input}' must look like 0x08000000-0x09000000"))?;
    let start_addr = parse_hex_u32(start)?;
    let end_addr = parse_hex_u32(end)?;
    if end_addr <= start_addr {
        return Err(format!(
            "range end 0x{end_addr:08X} must be greater than start 0x{start_addr:08X}"
        ));
    }
    Ok(start_addr..end_addr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_addresses() {
        assert_eq!(parse_hex_u32("0x00100000").unwrap(), 0x0010_0000);
        assert_eq!(parse_hex_u32("D2CAA0").unwrap(), 0x00D2_CAA0);
        assert_eq!(
            parse_hex_u64("0x0004000000197100").unwrap(),
            0x0004_0000_0019_7100
        );
    }

    #[test]
    fn parses_ranges() {
        let range = parse_range("0x08000000-0x09000000").unwrap();
        assert_eq!(range.start, 0x0800_0000);
        assert_eq!(range.end, 0x0900_0000);
        assert!(parse_range("0x20-0x10").is_err());
    }
}
