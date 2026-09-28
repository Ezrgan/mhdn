use std::collections::HashMap;
use std::ops::Range;

use mhdn_rpc::MemorySource;

use crate::error::{ProbeError, Result};

/// `base` is a guest address that holds a pointer. Each offset is added after a
/// dereference, except the last offset, which is only added. That matches the
/// usual pointer path `static +off +off +off`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointerPath {
    pub base: u32,
    pub offsets: Vec<u32>,
}

pub struct PtrScanConfig {
    pub depth: u32,
    pub max_offset: u32,
    pub static_range: Range<u32>,
    pub max_paths: usize,
}

pub fn scan_pointers(
    image_base: u32,
    image: &[u8],
    target: u32,
    config: &PtrScanConfig,
) -> Vec<PointerPath> {
    let index = build_index(image_base, image);
    let mut found = Vec::new();
    let mut calls = 0usize;
    search(
        &index,
        target,
        config.depth,
        config,
        &mut Vec::new(),
        &mut found,
        &mut calls,
    );
    found
}

pub fn format_path(path: &PointerPath) -> String {
    let mut out = format!("0x{:08X}", path.base);
    for offset in &path.offsets {
        out.push_str(&format!(" +0x{offset:X}"));
    }
    out
}

pub fn parse_path(input: &str) -> std::result::Result<PointerPath, String> {
    let parts: Vec<&str> = input
        .split('+')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return Err("pointer path is empty".to_string());
    }
    let base = crate::parse::parse_hex_u32(parts[0])?;
    let mut offsets = Vec::with_capacity(parts.len().saturating_sub(1));
    for part in &parts[1..] {
        let offset = crate::parse::parse_hex_u32(part)?;
        offsets.push(offset);
    }
    Ok(PointerPath { base, offsets })
}

pub fn resolve_in_image(image_base: u32, image: &[u8], path: &PointerPath) -> Option<u32> {
    if path.offsets.is_empty() {
        return Some(path.base);
    }
    let mut addr = read_u32(image_base, image, path.base)?;
    let last = path.offsets.len() - 1;
    for (index, offset) in path.offsets.iter().enumerate() {
        let next = addr.checked_add(*offset)?;
        if index == last {
            return Some(next);
        }
        addr = read_u32(image_base, image, next)?;
    }
    None
}

pub fn resolve_memory(mem: &mut dyn MemorySource, path: &PointerPath) -> Result<u32> {
    if path.offsets.is_empty() {
        return Ok(path.base);
    }
    let mut addr = mem.read_u32(path.base)?;
    let last = path.offsets.len() - 1;
    for (index, offset) in path.offsets.iter().enumerate() {
        let next = addr.checked_add(*offset).ok_or_else(|| {
            ProbeError::msg(format!(
                "pointer path overflow at 0x{addr:08X}+0x{offset:X}"
            ))
        })?;
        if index == last {
            return Ok(next);
        }
        addr = mem.read_u32(next)?;
    }
    Err(ProbeError::msg("pointer path was empty after the base"))
}

fn search(
    index: &HashMap<u32, Vec<u32>>,
    goal: u32,
    depth_left: u32,
    config: &PtrScanConfig,
    offsets_rev: &mut Vec<u32>,
    found: &mut Vec<PointerPath>,
    calls: &mut usize,
) {
    const MAX_CALLS: usize = 100_000;
    if depth_left == 0 || found.len() >= config.max_paths || *calls >= MAX_CALLS {
        return;
    }
    *calls += 1;
    let parents = parents_of(index, goal, config.max_offset);
    for (location, offset) in parents {
        if found.len() >= config.max_paths {
            return;
        }
        offsets_rev.push(offset);
        if config.static_range.contains(&location) {
            let mut offsets = offsets_rev.clone();
            offsets.reverse();
            found.push(PointerPath {
                base: location,
                offsets,
            });
        }
        if depth_left > 1 && location != goal {
            search(
                index,
                location,
                depth_left - 1,
                config,
                offsets_rev,
                found,
                calls,
            );
        }
        offsets_rev.pop();
    }
}

fn parents_of(index: &HashMap<u32, Vec<u32>>, goal: u32, max_offset: u32) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let mut offset = 0u32;
    while offset <= max_offset {
        let Some(pointed) = goal.checked_sub(offset) else {
            break;
        };
        if let Some(locations) = index.get(&pointed) {
            for location in locations {
                out.push((*location, offset));
                if out.len() >= 256 {
                    return out;
                }
            }
        }
        offset = offset.saturating_add(4);
    }
    out
}

fn build_index(image_base: u32, image: &[u8]) -> HashMap<u32, Vec<u32>> {
    let mut index: HashMap<u32, Vec<u32>> = HashMap::new();
    let image_end = image_base.saturating_add(image.len() as u32);
    let mut addr = align_up(image_base);
    while addr.saturating_add(4) <= image_end {
        if let Some(value) = read_u32(image_base, image, addr) {
            if value != 0 && value >= image_base && value < image_end && value.is_multiple_of(4) {
                let locations = index.entry(value).or_default();
                if locations.len() < 32 {
                    locations.push(addr);
                }
            }
        }
        addr = addr.saturating_add(4);
    }
    index
}

fn align_up(addr: u32) -> u32 {
    let mis = addr % 4;
    if mis == 0 {
        addr
    } else {
        addr.saturating_add(4 - mis)
    }
}

fn read_u32(image_base: u32, image: &[u8], addr: u32) -> Option<u32> {
    let offset = addr.checked_sub(image_base)? as usize;
    let bytes = image.get(offset..offset + 4)?;
    Some(u32::from_le_bytes(bytes.try_into().expect("4 bytes")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mhdn_rpc::FileMemorySource;

    fn put_u32(image: &mut [u8], image_base: u32, addr: u32, value: u32) {
        let offset = (addr - image_base) as usize;
        image[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    /// The documented MHXX monster chain: static → +0x14 → +0x10A8 → +0x360 = HP.
    #[test]
    fn rediscovers_the_known_monster_pointer_chain() {
        let image_base = 0x0010_0000;
        let mut image = vec![0u8; 0x6000];
        let static_addr = 0x0010_1000;
        put_u32(&mut image, image_base, static_addr, 0x0010_2000);
        put_u32(&mut image, image_base, 0x0010_2014, 0x0010_3000);
        put_u32(&mut image, image_base, 0x0010_40A8, 0x0010_5000);
        let target = 0x0010_5360;

        let found = scan_pointers(
            image_base,
            &image,
            target,
            &PtrScanConfig {
                depth: 4,
                max_offset: 0x2000,
                static_range: static_addr..static_addr + 4,
                max_paths: 16,
            },
        );
        assert!(
            found
                .iter()
                .any(|path| { path.base == static_addr && path.offsets == [0x14, 0x10A8, 0x360] }),
            "paths: {found:?}"
        );
        let path = found
            .iter()
            .find(|path| path.offsets == [0x14, 0x10A8, 0x360])
            .unwrap();
        assert_eq!(resolve_in_image(image_base, &image, path), Some(target));

        let mut mem = FileMemorySource::from_bytes(image, image_base);
        assert_eq!(resolve_memory(&mut mem, path).unwrap(), target);
    }

    #[test]
    fn depth_two_cannot_reach_a_three_deref_chain() {
        let image_base = 0x0010_0000;
        let mut image = vec![0u8; 0x6000];
        put_u32(&mut image, image_base, 0x0010_1000, 0x0010_2000);
        put_u32(&mut image, image_base, 0x0010_2014, 0x0010_3000);
        put_u32(&mut image, image_base, 0x0010_40A8, 0x0010_5000);
        let found = scan_pointers(
            image_base,
            &image,
            0x0010_5360,
            &PtrScanConfig {
                depth: 2,
                max_offset: 0x2000,
                static_range: 0x0010_1000..0x0010_1004,
                max_paths: 16,
            },
        );
        assert!(found.is_empty());
    }

    #[test]
    fn parses_and_formats_a_path() {
        let path = parse_path("0xD2CAA0+0x14+0x10A8+0x360").unwrap();
        assert_eq!(path.base, 0x00D2_CAA0);
        assert_eq!(path.offsets, vec![0x14, 0x10A8, 0x360]);
        assert_eq!(format_path(&path), "0x00D2CAA0 +0x14 +0x10A8 +0x360");
    }
}
