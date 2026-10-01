use std::time::{Duration, Instant};

use mhdn_game::{ChainSpec, FieldRef, FieldSpec, FieldType, Profile, SceneFlag};
use mhdn_rpc::MemorySource;

use crate::error::{ProbeError, Result};

const MAGIC: &[u8; 4] = b"MHRC";
const VERSION: u32 = 1;
const MAX_MONSTERS: usize = 16;

#[derive(Debug, Clone)]
pub struct RecordedMonster {
    pub addr: u32,
    pub hp: u32,
    pub max_hp: u32,
    pub species: u16,
    pub pos: [f32; 3],
}

#[derive(Debug, Clone)]
pub struct RecordedFrame {
    pub host_us: u64,
    pub guest_frame: u32,
    pub scene: u32,
    pub hunter: [f32; 3],
    pub eye: [f32; 3],
    pub target: [f32; 3],
    pub fov: f32,
    pub monsters: Vec<RecordedMonster>,
}

#[derive(Debug, Clone)]
pub struct Recording {
    pub profile_id: String,
    pub hz: u32,
    pub frames: Vec<RecordedFrame>,
}

pub fn encode(recording: &Recording) -> Result<Vec<u8>> {
    let id = recording.profile_id.as_bytes();
    if id.len() > u16::MAX as usize {
        return Err(ProbeError::msg("profile id is too long"));
    }
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(id.len() as u16).to_le_bytes());
    out.extend_from_slice(id);
    out.extend_from_slice(&recording.hz.to_le_bytes());
    let count = u32::try_from(recording.frames.len())
        .map_err(|_| ProbeError::msg("too many recorded frames"))?;
    out.extend_from_slice(&count.to_le_bytes());
    for frame in &recording.frames {
        if frame.monsters.len() > MAX_MONSTERS {
            return Err(ProbeError::msg("a frame has more than 16 monsters"));
        }
        out.extend_from_slice(&frame.host_us.to_le_bytes());
        out.extend_from_slice(&frame.guest_frame.to_le_bytes());
        out.extend_from_slice(&frame.scene.to_le_bytes());
        write_f32x3(&mut out, frame.hunter);
        write_f32x3(&mut out, frame.eye);
        write_f32x3(&mut out, frame.target);
        out.extend_from_slice(&frame.fov.to_le_bytes());
        out.extend_from_slice(&(frame.monsters.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        for monster in &frame.monsters {
            out.extend_from_slice(&monster.addr.to_le_bytes());
            out.extend_from_slice(&monster.hp.to_le_bytes());
            out.extend_from_slice(&monster.max_hp.to_le_bytes());
            out.extend_from_slice(&monster.species.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            write_f32x3(&mut out, monster.pos);
        }
    }
    Ok(out)
}

pub fn decode(bytes: &[u8]) -> Result<Recording> {
    let mut cursor = 0usize;
    let magic = read_array::<4>(bytes, &mut cursor)?;
    if &magic != MAGIC {
        return Err(ProbeError::msg("not an .mhrec file"));
    }
    let version = read_u32(bytes, &mut cursor)?;
    if version != VERSION {
        return Err(ProbeError::msg(format!(
            "unsupported .mhrec version {version}"
        )));
    }
    let id_len = read_u16(bytes, &mut cursor)? as usize;
    let id = read_bytes(bytes, &mut cursor, id_len)?;
    let profile_id = std::str::from_utf8(id)
        .map_err(|_| ProbeError::msg(".mhrec profile id is not utf-8"))?
        .to_string();
    let hz = read_u32(bytes, &mut cursor)?;
    let frame_count = read_u32(bytes, &mut cursor)? as usize;
    let mut frames = Vec::with_capacity(frame_count);
    for _ in 0..frame_count {
        let host_us = read_u64(bytes, &mut cursor)?;
        let guest_frame = read_u32(bytes, &mut cursor)?;
        let scene = read_u32(bytes, &mut cursor)?;
        let hunter = read_f32x3(bytes, &mut cursor)?;
        let eye = read_f32x3(bytes, &mut cursor)?;
        let target = read_f32x3(bytes, &mut cursor)?;
        let fov = f32::from_le_bytes(read_array(bytes, &mut cursor)?);
        let monster_count = read_u16(bytes, &mut cursor)? as usize;
        let _pad = read_u16(bytes, &mut cursor)?;
        if monster_count > MAX_MONSTERS {
            return Err(ProbeError::msg("recorded frame has more than 16 monsters"));
        }
        let mut monsters = Vec::with_capacity(monster_count);
        for _ in 0..monster_count {
            monsters.push(RecordedMonster {
                addr: read_u32(bytes, &mut cursor)?,
                hp: read_u32(bytes, &mut cursor)?,
                max_hp: read_u32(bytes, &mut cursor)?,
                species: read_u16(bytes, &mut cursor)?,
                pos: {
                    let _pad = read_u16(bytes, &mut cursor)?;
                    read_f32x3(bytes, &mut cursor)?
                },
            });
        }
        frames.push(RecordedFrame {
            host_us,
            guest_frame,
            scene,
            hunter,
            eye,
            target,
            fov,
            monsters,
        });
    }
    Ok(Recording {
        profile_id,
        hz,
        frames,
    })
}

pub fn capture_frame(
    mem: &mut dyn MemorySource,
    profile: &Profile,
    host_us: u64,
) -> Result<RecordedFrame> {
    Ok(RecordedFrame {
        host_us,
        guest_frame: read_guest_frame(mem, profile)?,
        scene: read_scene(mem, profile)?,
        hunter: read_hunter(mem, profile)?,
        eye: read_camera_vec3(mem, profile, |camera| camera.eye.as_ref())?,
        target: read_camera_vec3(mem, profile, |camera| camera.target.as_ref())?,
        fov: read_camera_fov(mem, profile)?,
        monsters: read_monsters(mem, profile)?,
    })
}

fn read_guest_frame(mem: &mut dyn MemorySource, profile: &Profile) -> Result<u32> {
    let Some(addr) = profile.frame_counter.addr else {
        return Ok(0);
    };
    Ok(mem.read_u32(addr)?)
}

fn read_scene(mem: &mut dyn MemorySource, profile: &Profile) -> Result<u32> {
    if let Some(flag) = &profile.scene.loading {
        if read_flag(mem, flag)? == flag.value {
            return Ok(2);
        }
    }
    if let Some(flag) = &profile.scene.in_quest {
        if read_flag(mem, flag)? == flag.value {
            return Ok(1);
        }
    }
    if profile.scene.in_quest.is_some() || profile.scene.loading.is_some() {
        return Ok(0);
    }
    Ok(u32::MAX)
}

fn read_flag(mem: &mut dyn MemorySource, flag: &SceneFlag) -> Result<u32> {
    match flag.ty {
        FieldType::U8 => {
            let mut buf = [0u8; 1];
            mem.read(flag.addr, &mut buf)?;
            Ok(u32::from(buf[0]))
        }
        FieldType::U16 => {
            let mut buf = [0u8; 2];
            mem.read(flag.addr, &mut buf)?;
            Ok(u32::from(u16::from_le_bytes(buf)))
        }
        FieldType::U32 => Ok(mem.read_u32(flag.addr)?),
        FieldType::F32 | FieldType::Vec3 => Err(ProbeError::msg("scene flag must be an integer")),
    }
}

fn deref_base(mem: &mut dyn MemorySource, base: Option<u32>) -> Result<Option<u32>> {
    let Some(addr) = base else {
        return Ok(None);
    };
    let ptr = mem.read_u32(addr)?;
    if ptr == 0 {
        return Ok(None);
    }
    Ok(Some(ptr))
}

fn read_hunter(mem: &mut dyn MemorySource, profile: &Profile) -> Result<[f32; 3]> {
    let Some(object) = deref_base(mem, profile.hunter.base)? else {
        return Ok([f32::NAN; 3]);
    };
    match &profile.hunter.pos {
        FieldRef::Relative(spec) if spec.ty == FieldType::Vec3 => read_vec3_field(mem, object, spec),
        _ => Ok([f32::NAN; 3]),
    }
}

fn read_camera_vec3(
    mem: &mut dyn MemorySource,
    profile: &Profile,
    pick: fn(&mhdn_game::CameraProfile) -> Option<&FieldRef>,
) -> Result<[f32; 3]> {
    let Some(object) = deref_base(mem, profile.camera.base)? else {
        return Ok([f32::NAN; 3]);
    };
    match pick(&profile.camera) {
        Some(FieldRef::Relative(spec)) if spec.ty == FieldType::Vec3 => {
            read_vec3_field(mem, object, spec)
        }
        _ => Ok([f32::NAN; 3]),
    }
}

fn read_camera_fov(mem: &mut dyn MemorySource, profile: &Profile) -> Result<f32> {
    let Some(object) = deref_base(mem, profile.camera.base)? else {
        return Ok(f32::NAN);
    };
    match &profile.camera.fov_y {
        Some(FieldRef::Relative(spec)) if spec.ty == FieldType::F32 => {
            let addr = add_offset(object, spec.off)
                .ok_or_else(|| ProbeError::msg("field offset overflow"))?;
            let mut buf = [0u8; 4];
            mem.read(addr, &mut buf)?;
            Ok(f32::from_le_bytes(buf))
        }
        _ => Ok(f32::NAN),
    }
}

pub fn record_for(
    mem: &mut dyn MemorySource,
    profile: &Profile,
    hz: u32,
    seconds: u64,
) -> Result<Recording> {
    if hz == 0 || hz > 120 {
        return Err(ProbeError::msg("record --hz must be between 1 and 120"));
    }
    let period = Duration::from_secs_f64(1.0 / f64::from(hz));
    let mut frames = Vec::new();
    let started = Instant::now();
    let deadline = started + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        let host_us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
        frames.push(capture_frame(mem, profile, host_us)?);
        std::thread::sleep(period);
    }
    Ok(Recording {
        profile_id: profile.id(),
        hz,
        frames,
    })
}

fn read_monsters(mem: &mut dyn MemorySource, profile: &Profile) -> Result<Vec<RecordedMonster>> {
    let ChainSpec::Offsets(chain) = &profile.monster_list.chain else {
        return Ok(Vec::new());
    };
    let FieldRef::Relative(hp) = &profile.monster.hp else {
        return Ok(Vec::new());
    };
    let FieldRef::Relative(max_hp) = &profile.monster.max_hp else {
        return Ok(Vec::new());
    };
    if hp.ty != FieldType::U32 || max_hp.ty != FieldType::U32 {
        return Err(ProbeError::msg("monster hp and max_hp must be u32"));
    }

    let mut list_base = None;
    for candidate in &profile.monster_list.base_candidates {
        let value = mem.read_u32(candidate.addr)?;
        if candidate.expect.contains(&value) {
            list_base = Some(value);
            break;
        }
    }
    let Some(list_base) = list_base else {
        return Ok(Vec::new());
    };

    let mut monsters = Vec::new();
    for slot in 0..profile.monster_list.slots.min(MAX_MONSTERS as u32) {
        let Some(slot_addr) =
            add_offset(list_base, profile.monster_list.slot_offset).and_then(|addr| {
                addr.checked_add(slot.saturating_mul(profile.monster_list.slot_stride))
            })
        else {
            continue;
        };
        let Ok(slot_ptr) = mem.read_u32(slot_addr) else {
            continue;
        };
        if slot_ptr == 0 {
            continue;
        }
        let Some(monster) = walk_chain(mem, slot_ptr, chain) else {
            continue;
        };
        let (Ok(hp_value), Ok(max_value)) = (
            read_u32_field(mem, monster, hp),
            read_u32_field(mem, monster, max_hp),
        ) else {
            continue;
        };
        if hp_value == 0 && max_value == 0 {
            continue;
        }
        let species = match &profile.monster.species {
            FieldRef::Relative(spec) if spec.ty == FieldType::U16 => {
                read_u16_field(mem, monster, spec).unwrap_or(0)
            }
            _ => 0,
        };
        let pos = match &profile.monster.pos {
            FieldRef::Relative(spec) if spec.ty == FieldType::Vec3 => {
                read_vec3_field(mem, monster, spec).unwrap_or([f32::NAN; 3])
            }
            _ => [f32::NAN; 3],
        };
        monsters.push(RecordedMonster {
            addr: monster,
            hp: hp_value,
            max_hp: max_value,
            species,
            pos,
        });
    }
    Ok(monsters)
}

fn walk_chain(mem: &mut dyn MemorySource, mut ptr: u32, chain: &[i32]) -> Option<u32> {
    if chain.is_empty() {
        return None;
    }
    let last = chain.len() - 1;
    for (index, offset) in chain.iter().enumerate() {
        let next = add_offset(ptr, *offset)?;
        if index == last {
            return Some(next);
        }
        ptr = mem.read_u32(next).ok()?;
        if ptr == 0 {
            return None;
        }
    }
    None
}

fn read_u32_field(mem: &mut dyn MemorySource, base: u32, spec: &FieldSpec) -> Result<u32> {
    let addr =
        add_offset(base, spec.off).ok_or_else(|| ProbeError::msg("field offset overflow"))?;
    Ok(mem.read_u32(addr)?)
}

fn read_u16_field(mem: &mut dyn MemorySource, base: u32, spec: &FieldSpec) -> Result<u16> {
    let addr =
        add_offset(base, spec.off).ok_or_else(|| ProbeError::msg("field offset overflow"))?;
    let mut buf = [0u8; 2];
    mem.read(addr, &mut buf)?;
    Ok(u16::from_le_bytes(buf))
}

fn read_vec3_field(mem: &mut dyn MemorySource, base: u32, spec: &FieldSpec) -> Result<[f32; 3]> {
    let addr =
        add_offset(base, spec.off).ok_or_else(|| ProbeError::msg("field offset overflow"))?;
    Ok(mem.read_f32x3(addr)?)
}

fn add_offset(addr: u32, offset: i32) -> Option<u32> {
    if offset >= 0 {
        addr.checked_add(offset as u32)
    } else {
        addr.checked_sub(offset.unsigned_abs())
    }
}

fn write_f32x3(out: &mut Vec<u8>, value: [f32; 3]) {
    for component in value {
        out.extend_from_slice(&component.to_le_bytes());
    }
}

fn read_array<const N: usize>(bytes: &[u8], cursor: &mut usize) -> Result<[u8; N]> {
    let slice = read_bytes(bytes, cursor, N)?;
    let mut array = [0u8; N];
    array.copy_from_slice(slice);
    Ok(array)
}

fn read_bytes<'a>(bytes: &'a [u8], cursor: &mut usize, len: usize) -> Result<&'a [u8]> {
    let end = cursor
        .checked_add(len)
        .ok_or_else(|| ProbeError::msg("truncated .mhrec"))?;
    if end > bytes.len() {
        return Err(ProbeError::msg("truncated .mhrec"));
    }
    let slice = &bytes[*cursor..end];
    *cursor = end;
    Ok(slice)
}

fn read_u16(bytes: &[u8], cursor: &mut usize) -> Result<u16> {
    Ok(u16::from_le_bytes(read_array(bytes, cursor)?))
}

fn read_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32> {
    Ok(u32::from_le_bytes(read_array(bytes, cursor)?))
}

fn read_u64(bytes: &[u8], cursor: &mut usize) -> Result<u64> {
    Ok(u64::from_le_bytes(read_array(bytes, cursor)?))
}

fn read_f32x3(bytes: &[u8], cursor: &mut usize) -> Result<[f32; 3]> {
    Ok([
        f32::from_le_bytes(read_array(bytes, cursor)?),
        f32::from_le_bytes(read_array(bytes, cursor)?),
        f32::from_le_bytes(read_array(bytes, cursor)?),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use mhdn_rpc::FileMemorySource;

    fn put_u32(image: &mut [u8], base: u32, addr: u32, value: u32) {
        let offset = (addr - base) as usize;
        image[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn capture_reads_hp_through_the_profile_chain() {
        let profile = Profile::from_toml_str(
            r#"
            [meta]
            game = "MHXX"
            region = "JP"
            version = "test"
            title_id = "0x1000"
            update_title_version = 1
            [monster_list]
            base_candidates = [{ addr = "0x1000", expect = ["0x2000"] }]
            slots = 2
            slot_stride = 4
            slot_offset = "0x14"
            chain = ["0x10", "0x20"]
            [monster]
            hp = { off = 0, ty = "u32" }
            max_hp = { off = 4, ty = "u32" }
            species = { off = 8, ty = "u16" }
            size = { off = -4, ty = "f32" }
            pos = "TBD"
            poison = { off = 12, ty = "u16" }
            visible_flag = { off = 14, ty = "u8" }
            [frame_counter]
            chain = "TBD"
            [scene]
            chain = "TBD"
            [hunter]
            pos = "TBD"
            [camera]
            mode = "params"
            fov_unit = "rad"
            [species]
            default_anchor_height = 150.0
            "#,
        )
        .unwrap();

        let base = 0x1000u32;
        let mut image = vec![0u8; 0x3100];
        put_u32(&mut image, base, 0x1000, 0x2000);
        put_u32(&mut image, base, 0x2014, 0x3000);
        put_u32(&mut image, base, 0x3010, 0x4000);
        put_u32(&mut image, base, 0x4020, 1500);
        put_u32(&mut image, base, 0x4024, 3000);
        image[0x4028 - base as usize] = 42;
        image[0x4029 - base as usize] = 0;

        let mut mem = FileMemorySource::from_bytes(image, base);
        let frame = capture_frame(&mut mem, &profile, 10).unwrap();
        assert_eq!(frame.monsters.len(), 1);
        assert_eq!(frame.monsters[0].addr, 0x4020);
        assert_eq!(frame.monsters[0].hp, 1500);
        assert_eq!(frame.monsters[0].max_hp, 3000);
        assert_eq!(frame.monsters[0].species, 42);
        assert!(frame.monsters[0].pos[0].is_nan());
        assert_eq!(frame.guest_frame, 0);
        assert_eq!(frame.scene, u32::MAX);
        assert!(frame.hunter[0].is_nan());
    }

    #[test]
    fn capture_reads_the_frame_counter_and_in_quest_flag() {
        let profile = Profile::from_toml_str(
            r#"
            [meta]
            game = "MHXX"
            region = "JP"
            version = "test"
            title_id = "0x1000"
            update_title_version = 1
            [monster_list]
            base_candidates = [{ addr = "0x1000", expect = ["0x2000"] }]
            slots = 1
            slot_stride = 4
            slot_offset = "0x14"
            chain = ["0x10"]
            [monster]
            hp = { off = 0, ty = "u32" }
            max_hp = { off = 4, ty = "u32" }
            species = "TBD"
            size = { off = -4, ty = "f32" }
            pos = "TBD"
            poison = { off = 12, ty = "u16" }
            visible_flag = { off = 14, ty = "u8" }
            [frame_counter]
            addr = "0x1200"
            chain = "TBD"
            [scene]
            chain = "TBD"
            in_quest = { addr = "0x1300", ty = "u32", value = 7 }
            loading = { addr = "0x1304", ty = "u8", value = 1 }
            [hunter]
            pos = "TBD"
            [camera]
            mode = "params"
            fov_unit = "rad"
            [species]
            default_anchor_height = 150.0
            "#,
        )
        .unwrap();
        let mut image = vec![0u8; 0x400];
        put_u32(&mut image, 0x1000, 0x1200, 42);
        put_u32(&mut image, 0x1000, 0x1300, 7);
        image[0x1304 - 0x1000] = 0;
        let mut mem = FileMemorySource::from_bytes(image, 0x1000);
        let frame = capture_frame(&mut mem, &profile, 1).unwrap();
        assert_eq!(frame.guest_frame, 42);
        assert_eq!(frame.scene, 1);
    }

    #[test]
    fn two_minutes_at_60hz_stays_under_10mb() {
        let monster = RecordedMonster {
            addr: 1,
            hp: 2,
            max_hp: 3,
            species: 4,
            pos: [1.0, 2.0, 3.0],
        };
        let frame = RecordedFrame {
            host_us: 1,
            guest_frame: 2,
            scene: 3,
            hunter: [1.0, 2.0, 3.0],
            eye: [4.0, 5.0, 6.0],
            target: [7.0, 8.0, 9.0],
            fov: 0.8,
            monsters: vec![monster; 16],
        };
        let recording = Recording {
            profile_id: "mhxx-jp-1.4-es".to_string(),
            hz: 60,
            frames: vec![frame; 120 * 60],
        };
        let bytes = encode(&recording).unwrap();
        assert!(bytes.len() < 10 * 1024 * 1024, "{}", bytes.len());
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded.frames.len(), 7200);
        assert_eq!(decoded.frames[0].monsters.len(), 16);
        assert_eq!(decoded.frames[0].monsters[0].hp, 2);
        assert_eq!(decoded.profile_id, "mhxx-jp-1.4-es");
    }
}
