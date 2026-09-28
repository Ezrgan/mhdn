use crate::error::{ProbeError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteChange {
    pub addr: u32,
    pub old: u8,
    pub new: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FloatChange {
    pub addr: u32,
    pub old: f32,
    pub new: f32,
}

pub fn changed_bytes(base: u32, prev: &[u8], next: &[u8]) -> Result<Vec<ByteChange>> {
    if prev.len() != next.len() {
        return Err(ProbeError::msg("watch buffers have different lengths"));
    }
    let mut changes = Vec::new();
    for (index, (old, new)) in prev.iter().zip(next.iter()).enumerate() {
        if old != new {
            changes.push(ByteChange {
                addr: base.wrapping_add(index as u32),
                old: *old,
                new: *new,
            });
        }
    }
    Ok(changes)
}

/// Aligned f32 words whose bits changed. Useful next to the raw byte diff.
pub fn changed_f32(base: u32, prev: &[u8], next: &[u8]) -> Result<Vec<FloatChange>> {
    if prev.len() != next.len() {
        return Err(ProbeError::msg("watch buffers have different lengths"));
    }
    let mut changes = Vec::new();
    let aligned = (4 - (base as usize % 4)) % 4;
    let mut offset = aligned;
    while offset + 4 <= prev.len() {
        let old_bytes: [u8; 4] = prev[offset..offset + 4].try_into().expect("4 bytes");
        let new_bytes: [u8; 4] = next[offset..offset + 4].try_into().expect("4 bytes");
        if old_bytes != new_bytes {
            changes.push(FloatChange {
                addr: base.wrapping_add(offset as u32),
                old: f32::from_le_bytes(old_bytes),
                new: f32::from_le_bytes(new_bytes),
            });
        }
        offset += 4;
    }
    Ok(changes)
}

pub fn format_diff(bytes: &[ByteChange], floats: &[FloatChange], limit: usize) -> String {
    let mut out = String::new();
    let shown_bytes = bytes.len().min(limit);
    for change in bytes.iter().take(shown_bytes) {
        out.push_str(&format!(
            "0x{:08X}  {:02X} -> {:02X}\n",
            change.addr, change.old, change.new
        ));
    }
    if bytes.len() > shown_bytes {
        out.push_str(&format!(
            "… {} more byte changes\n",
            bytes.len() - shown_bytes
        ));
    }
    for change in floats {
        out.push_str(&format!(
            "0x{:08X}  f32 {} -> {}\n",
            change.addr, change.old, change.new
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_the_bytes_and_float_that_moved() {
        let prev = 10.0f32.to_le_bytes();
        let mut next = prev;
        next.copy_from_slice(&12.5f32.to_le_bytes());
        let bytes = changed_bytes(0x2000, &prev, &next).unwrap();
        assert!(!bytes.is_empty());
        let floats = changed_f32(0x2000, &prev, &next).unwrap();
        assert_eq!(floats.len(), 1);
        assert_eq!(floats[0].addr, 0x2000);
        assert_eq!(floats[0].new, 12.5);
    }

    #[test]
    fn quiet_region_has_no_changes() {
        let buf = [1u8, 2, 3, 4];
        assert!(changed_bytes(0, &buf, &buf).unwrap().is_empty());
        assert!(changed_f32(0, &buf, &buf).unwrap().is_empty());
    }
}
