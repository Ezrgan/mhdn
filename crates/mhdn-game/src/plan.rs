//! Merge nearby guest reads into blocks of at most 1 KiB.

/// Fuse ranges that sit within `max_gap` bytes when the fused block is at most `max_len`.
pub fn coalesce(mut ranges: Vec<(u32, u32)>, max_len: u32, max_gap: u32) -> Vec<(u32, u32)> {
    ranges.retain(|(_, len)| *len > 0);
    ranges.sort_by_key(|range| range.0);
    let mut out: Vec<(u32, u32)> = Vec::new();
    for (start, len) in ranges {
        let end = start.saturating_add(len);
        if let Some((block_start, block_len)) = out.last_mut() {
            let block_end = block_start.saturating_add(*block_len);
            let gap = start.saturating_sub(block_end);
            let merged = end.saturating_sub(*block_start);
            if start >= *block_start && gap <= max_gap && merged <= max_len {
                *block_len = merged;
                continue;
            }
        }
        out.push((start, end.saturating_sub(start)));
    }
    out
}

pub const MAX_BLOCK: u32 = 1024;
pub const MAX_GAP: u32 = 1024;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_ranges_that_sit_under_a_kilobyte_apart() {
        let fused = coalesce(vec![(100, 4), (0, 8), (2000, 4)], MAX_BLOCK, MAX_GAP);
        assert_eq!(fused, vec![(0, 104), (2000, 4)]);
    }

    #[test]
    fn refuses_a_block_larger_than_the_rpc_payload() {
        let fused = coalesce(vec![(0, 4), (800, 400)], MAX_BLOCK, MAX_GAP);
        assert_eq!(fused.len(), 2);
    }
}
