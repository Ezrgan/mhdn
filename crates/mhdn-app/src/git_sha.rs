//! Turn `git rev-parse --short HEAD` output into the build stamp. A missing git,
//! a failed command, or a non-hex answer becomes `unknown` so the build still finishes.

#[allow(dead_code)]
pub fn normalize(stdout: &[u8], success: bool) -> String {
    if !success {
        return "unknown".to_string();
    }
    let text = String::from_utf8_lossy(stdout);
    let sha = text.trim();
    if sha.is_empty() || sha.len() > 40 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        "unknown".to_string()
    } else {
        sha.to_string()
    }
}

#[allow(dead_code)]
pub fn format_build(sha: &str) -> String {
    format!("v0.4.0-dev+{sha}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_or_dirty_sha_falls_back_to_unknown() {
        assert_eq!(normalize(b"", false), "unknown");
        assert_eq!(normalize(b"", true), "unknown");
        assert_eq!(normalize(b"   \n", true), "unknown");
        assert_eq!(normalize(b"not-a-sha\n", true), "unknown");
        assert_eq!(normalize(b"abc def", true), "unknown");
        assert_eq!(normalize(b"e55cdc8\n", true), "e55cdc8");
    }

    #[test]
    fn the_build_stamp_uses_the_dev_prefix() {
        assert_eq!(format_build("e55cdc8"), "v0.4.0-dev+e55cdc8");
        assert_eq!(format_build("unknown"), "v0.4.0-dev+unknown");
    }
}
