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
}
