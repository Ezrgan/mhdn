use std::fs::File;
use std::io::{Read, Write};
use std::ops::Range;
use std::path::Path;

use crate::error::{ProbeError, Result};

const MAGIC: &[u8; 4] = b"MHSC";
const VERSION: u32 = 1;
const F32_EPS: f32 = 1.0e-4;
pub const DEFAULT_SESSION: &str = "scan.mhscan";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    U16,
    U32,
    F32,
}

impl ValueType {
    pub fn parse(name: &str) -> std::result::Result<Self, String> {
        match name.to_ascii_lowercase().as_str() {
            "u16" => Ok(Self::U16),
            "u32" => Ok(Self::U32),
            "f32" | "float" => Ok(Self::F32),
            other => Err(format!(
                "unknown scan type '{other}' (expected u16, u32, f32)"
            )),
        }
    }

    fn width(self) -> usize {
        match self {
            Self::U16 => 2,
            Self::U32 | Self::F32 => 4,
        }
    }

    fn tag(self) -> u8 {
        match self {
            Self::U16 => 0,
            Self::U32 => 1,
            Self::F32 => 2,
        }
    }

    fn from_tag(tag: u8) -> Result<Self> {
        match tag {
            0 => Ok(Self::U16),
            1 => Ok(Self::U32),
            2 => Ok(Self::F32),
            other => Err(ProbeError::msg(format!("unknown scan type tag {other}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InitialQuery {
    Unknown,
    Equal(u32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Filter {
    Eq(u32),
    Ne(u32),
    Gt(u32),
    Lt(u32),
    Inc,
    Dec,
    Changed,
    Unchanged,
}

impl Filter {
    pub fn parse(
        name: &str,
        ty: ValueType,
        value: Option<&str>,
    ) -> std::result::Result<Self, String> {
        let needs_value = matches!(
            name.to_ascii_lowercase().as_str(),
            "eq" | "ne" | "gt" | "lt"
        );
        let bits = if needs_value {
            let raw = value.ok_or_else(|| format!("filter '{name}' needs a value"))?;
            Some(parse_scan_value(ty, raw)?)
        } else if value.is_some() {
            return Err(format!("filter '{name}' does not take a value"));
        } else {
            None
        };
        match name.to_ascii_lowercase().as_str() {
            "eq" => Ok(Self::Eq(bits.unwrap())),
            "ne" => Ok(Self::Ne(bits.unwrap())),
            "gt" => Ok(Self::Gt(bits.unwrap())),
            "lt" => Ok(Self::Lt(bits.unwrap())),
            "inc" => Ok(Self::Inc),
            "dec" => Ok(Self::Dec),
            "changed" => Ok(Self::Changed),
            "unchanged" => Ok(Self::Unchanged),
            other => Err(format!(
                "unknown filter '{other}' (expected eq, ne, gt, lt, inc, dec, changed, unchanged)"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub addr: u32,
    pub prev: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanSnapshot {
    pub ty: ValueType,
    pub range: Range<u32>,
    pub candidates: Vec<Candidate>,
}

pub fn parse_scan_value(ty: ValueType, raw: &str) -> std::result::Result<u32, String> {
    match ty {
        ValueType::U16 => {
            let value = parse_int(raw)?;
            u16::try_from(value)
                .map(u32::from)
                .map_err(|_| format!("value '{raw}' does not fit in u16"))
        }
        ValueType::U32 => {
            let value = parse_int(raw)?;
            u32::try_from(value).map_err(|_| format!("value '{raw}' does not fit in u32"))
        }
        ValueType::F32 => {
            let value: f32 = raw
                .parse()
                .map_err(|_| format!("value '{raw}' is not an f32"))?;
            Ok(value.to_bits())
        }
    }
}

fn parse_int(raw: &str) -> std::result::Result<u64, String> {
    let s = raw.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).map_err(|e| format!("invalid hex '{raw}': {e}"))
    } else {
        s.parse::<u64>()
            .map_err(|e| format!("invalid integer '{raw}': {e}"))
    }
}

pub fn format_value(ty: ValueType, bits: u32) -> String {
    match ty {
        ValueType::U16 => {
            let value = bits as u16;
            format!("{value} (0x{value:04X})")
        }
        ValueType::U32 => format!("{bits} (0x{bits:08X})"),
        ValueType::F32 => format!("{}", f32::from_bits(bits)),
    }
}

/// First scan over `data`, which is the bytes at guest addresses `range`.
pub fn scan_new(
    range: Range<u32>,
    data: &[u8],
    ty: ValueType,
    query: InitialQuery,
) -> Result<ScanSnapshot> {
    check_coverage(range.clone(), data)?;
    let width = ty.width();
    if !range.start.is_multiple_of(width as u32) {
        return Err(ProbeError::msg(format!(
            "range start 0x{:08X} is not aligned to {width}",
            range.start
        )));
    }
    let mut candidates = Vec::new();
    let mut addr = range.start;
    while addr as usize + width <= range.end as usize {
        let bits = read_bits(range.start, data, addr, ty)?;
        let keep = match query {
            InitialQuery::Unknown => true,
            InitialQuery::Equal(expected) => values_equal(ty, bits, expected),
        };
        if keep {
            candidates.push(Candidate { addr, prev: bits });
        }
        addr = addr.saturating_add(width as u32);
    }
    Ok(ScanSnapshot {
        ty,
        range,
        candidates,
    })
}

pub fn scan_next(previous: &ScanSnapshot, data: &[u8], filter: Filter) -> Result<ScanSnapshot> {
    check_coverage(previous.range.clone(), data)?;
    let mut candidates = Vec::new();
    for candidate in &previous.candidates {
        let bits = read_bits(previous.range.start, data, candidate.addr, previous.ty)?;
        if passes(previous.ty, candidate.prev, bits, filter) {
            candidates.push(Candidate {
                addr: candidate.addr,
                prev: bits,
            });
        }
    }
    Ok(ScanSnapshot {
        ty: previous.ty,
        range: previous.range.clone(),
        candidates,
    })
}

pub fn save_snapshot(path: &Path, snapshot: &ScanSnapshot) -> Result<()> {
    let mut file = File::create(path)?;
    file.write_all(MAGIC)?;
    file.write_all(&VERSION.to_le_bytes())?;
    file.write_all(&[snapshot.ty.tag(), 0, 0, 0])?;
    file.write_all(&snapshot.range.start.to_le_bytes())?;
    file.write_all(&snapshot.range.end.to_le_bytes())?;
    let count = u32::try_from(snapshot.candidates.len())
        .map_err(|_| ProbeError::msg("too many scan candidates"))?;
    file.write_all(&count.to_le_bytes())?;
    for candidate in &snapshot.candidates {
        file.write_all(&candidate.addr.to_le_bytes())?;
        file.write_all(&candidate.prev.to_le_bytes())?;
    }
    Ok(())
}

pub fn load_snapshot(path: &Path) -> Result<ScanSnapshot> {
    let mut file = File::open(path)?;
    let mut header = [0u8; 24];
    file.read_exact(&mut header).map_err(|_| {
        ProbeError::msg(format!(
            "scan session {} is truncated or not an MHSC file",
            path.display()
        ))
    })?;
    if &header[0..4] != MAGIC {
        return Err(ProbeError::msg(format!(
            "{} is not a scan session (missing MHSC magic)",
            path.display()
        )));
    }
    let version = u32::from_le_bytes(header[4..8].try_into().expect("4 bytes"));
    if version != VERSION {
        return Err(ProbeError::msg(format!(
            "unsupported scan session version {version}"
        )));
    }
    let ty = ValueType::from_tag(header[8])?;
    let start = u32::from_le_bytes(header[12..16].try_into().expect("4 bytes"));
    let end = u32::from_le_bytes(header[16..20].try_into().expect("4 bytes"));
    let count = u32::from_le_bytes(header[20..24].try_into().expect("4 bytes"));
    if count > 32_000_000 {
        return Err(ProbeError::msg(format!(
            "scan session claims {count} candidates, which is above the 32M cap"
        )));
    }
    let count = count as usize;
    let mut candidates = Vec::with_capacity(count);
    for _ in 0..count {
        let mut rec = [0u8; 8];
        file.read_exact(&mut rec)?;
        candidates.push(Candidate {
            addr: u32::from_le_bytes(rec[0..4].try_into().expect("4 bytes")),
            prev: u32::from_le_bytes(rec[4..8].try_into().expect("4 bytes")),
        });
    }
    Ok(ScanSnapshot {
        ty,
        range: start..end,
        candidates,
    })
}

fn check_coverage(range: Range<u32>, data: &[u8]) -> Result<()> {
    let expected = range
        .end
        .checked_sub(range.start)
        .ok_or_else(|| ProbeError::msg("scan range wraps"))? as usize;
    if data.len() != expected {
        return Err(ProbeError::msg(format!(
            "memory image is {} bytes but the scan range is {expected} bytes",
            data.len()
        )));
    }
    Ok(())
}

fn read_bits(base: u32, data: &[u8], addr: u32, ty: ValueType) -> Result<u32> {
    let offset = addr
        .checked_sub(base)
        .ok_or_else(|| ProbeError::msg(format!("address 0x{addr:08X} is below the scan base")))?
        as usize;
    let width = ty.width();
    let end = offset
        .checked_add(width)
        .ok_or_else(|| ProbeError::msg("scan read overflow"))?;
    if end > data.len() {
        return Err(ProbeError::msg(format!(
            "address 0x{addr:08X} is outside the scanned range"
        )));
    }
    let bytes = &data[offset..end];
    Ok(match ty {
        ValueType::U16 => u32::from(u16::from_le_bytes([bytes[0], bytes[1]])),
        ValueType::U32 | ValueType::F32 => u32::from_le_bytes(bytes.try_into().expect("4 bytes")),
    })
}

fn passes(ty: ValueType, prev: u32, current: u32, filter: Filter) -> bool {
    match filter {
        Filter::Eq(value) => values_equal(ty, current, value),
        Filter::Ne(value) => !values_equal(ty, current, value),
        Filter::Gt(value) => greater(ty, current, value),
        Filter::Lt(value) => greater(ty, value, current),
        Filter::Inc => greater(ty, current, prev),
        Filter::Dec => greater(ty, prev, current),
        Filter::Changed => !values_equal(ty, current, prev),
        Filter::Unchanged => values_equal(ty, current, prev),
    }
}

fn values_equal(ty: ValueType, a: u32, b: u32) -> bool {
    match ty {
        ValueType::U16 | ValueType::U32 => a == b,
        ValueType::F32 => {
            let (a, b) = (f32::from_bits(a), f32::from_bits(b));
            if a.is_nan() && b.is_nan() {
                return true;
            }
            (a - b).abs() <= F32_EPS
        }
    }
}

fn greater(ty: ValueType, a: u32, b: u32) -> bool {
    match ty {
        ValueType::U16 | ValueType::U32 => a > b,
        ValueType::F32 => {
            let (a, b) = (f32::from_bits(a), f32::from_bits(b));
            a > b + F32_EPS
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_u32(buf: &mut [u8], addr: usize, value: u32) {
        buf[addr..addr + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn narrows_a_blind_hp_scan_to_the_value_that_dropped() {
        let mut before = vec![0u8; 0x300];
        put_u32(&mut before, 0x000, 500);
        put_u32(&mut before, 0x100, 500);
        put_u32(&mut before, 0x200, 500);
        let first = scan_new(0..0x300, &before, ValueType::U32, InitialQuery::Equal(500)).unwrap();
        assert_eq!(first.candidates.len(), 3);

        let mut after = before.clone();
        put_u32(&mut after, 0x100, 460);
        let second = scan_next(&first, &after, Filter::Lt(500)).unwrap();
        assert_eq!(second.candidates.len(), 1);
        assert_eq!(second.candidates[0].addr, 0x100);
        assert_eq!(second.candidates[0].prev, 460);
    }

    #[test]
    fn float_inc_keeps_only_the_axis_that_moved() {
        let mut before = vec![0u8; 12];
        before[0..4].copy_from_slice(&1.0f32.to_le_bytes());
        before[4..8].copy_from_slice(&2.0f32.to_le_bytes());
        before[8..12].copy_from_slice(&3.0f32.to_le_bytes());
        let first = scan_new(0..12, &before, ValueType::F32, InitialQuery::Unknown).unwrap();
        assert_eq!(first.candidates.len(), 3);

        let mut after = before.clone();
        after[0..4].copy_from_slice(&1.5f32.to_le_bytes());
        let second = scan_next(&first, &after, Filter::Inc).unwrap();
        assert_eq!(second.candidates.len(), 1);
        assert_eq!(second.candidates[0].addr, 0);
    }

    #[test]
    fn snapshot_roundtrip() {
        let snapshot = ScanSnapshot {
            ty: ValueType::U16,
            range: 0x1000..0x2000,
            candidates: vec![Candidate {
                addr: 0x1000,
                prev: 7,
            }],
        };
        let path = std::env::temp_dir().join(format!("mhdn-scan-{}.mhscan", std::process::id()));
        save_snapshot(&path, &snapshot).unwrap();
        let loaded = load_snapshot(&path).unwrap();
        assert_eq!(loaded, snapshot);
        let _ = std::fs::remove_file(&path);
    }
}
