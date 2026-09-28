/// Classic 16-byte hex dump. Guest addresses are printed in the left column.
pub fn hexdump(base: u32, data: &[u8]) -> String {
    let mut out = String::new();
    if data.is_empty() {
        return out;
    }
    for (row, chunk) in data.chunks(16).enumerate() {
        if row > 0 {
            out.push('\n');
        }
        let addr = base.wrapping_add((row * 16) as u32);
        out.push_str(&format!("{addr:08X}  "));
        for i in 0..16 {
            if i == 8 {
                out.push(' ');
            }
            if let Some(byte) = chunk.get(i) {
                out.push_str(&format!("{byte:02X} "));
            } else {
                out.push_str("   ");
            }
        }
        out.push(' ');
        for byte in chunk {
            let ch = if byte.is_ascii_graphic() {
                *byte as char
            } else {
                '.'
            };
            out.push(ch);
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeekType {
    U8,
    U16,
    U32,
    F32,
    Vec3,
}

impl PeekType {
    pub fn parse(name: &str) -> std::result::Result<Self, String> {
        match name.to_ascii_lowercase().as_str() {
            "u8" | "byte" => Ok(Self::U8),
            "u16" => Ok(Self::U16),
            "u32" => Ok(Self::U32),
            "f32" | "float" => Ok(Self::F32),
            "vec3" | "f32x3" => Ok(Self::Vec3),
            other => Err(format!(
                "unknown peek type '{other}' (expected u8, u16, u32, f32, vec3)"
            )),
        }
    }

    pub fn size(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::U16 => 2,
            Self::U32 | Self::F32 => 4,
            Self::Vec3 => 12,
        }
    }
}

pub fn format_peek(ty: PeekType, addr: u32, data: &[u8]) -> String {
    match ty {
        PeekType::U8 => format!("u8 @ 0x{addr:08X} = {}", data[0]),
        PeekType::U16 => {
            let value = u16::from_le_bytes([data[0], data[1]]);
            format!("u16 @ 0x{addr:08X} = {value} (0x{value:04X})")
        }
        PeekType::U32 => {
            let value = u32::from_le_bytes(data[0..4].try_into().expect("4 bytes"));
            format!("u32 @ 0x{addr:08X} = {value} (0x{value:08X})")
        }
        PeekType::F32 => {
            let value = f32::from_le_bytes(data[0..4].try_into().expect("4 bytes"));
            format!("f32 @ 0x{addr:08X} = {value}")
        }
        PeekType::Vec3 => {
            let x = f32::from_le_bytes(data[0..4].try_into().expect("4 bytes"));
            let y = f32::from_le_bytes(data[4..8].try_into().expect("4 bytes"));
            let z = f32::from_le_bytes(data[8..12].try_into().expect("4 bytes"));
            format!("vec3 @ 0x{addr:08X} = ({x}, {y}, {z})")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hexdump_row_and_ascii() {
        let data = b"\x00\x11ARM\x7f\xff";
        let text = hexdump(0x0010_0000, data);
        assert!(text.starts_with("00100000  00 11 41 52 4D 7F FF"));
        assert!(text.ends_with(".ARM.."));
    }

    #[test]
    fn peek_formats_little_endian() {
        let raw = 0x1234_5678u32.to_le_bytes();
        let text = format_peek(PeekType::U32, 0x0800_0000, &raw);
        assert!(text.contains("305419896"));
        assert!(text.contains("0x12345678"));
    }
}
