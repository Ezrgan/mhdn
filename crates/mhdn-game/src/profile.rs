use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;

use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use thiserror::Error;

pub const MHXX_JP_TITLE_ID: u64 = 0x0004_0000_0019_7100;

#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("failed to read profile: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid profile TOML: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("invalid profile: {}", .0.join("; "))]
    Invalid(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub meta: Meta,
    pub monster_list: MonsterList,
    pub monster: MonsterLayout,
    pub frame_counter: ChainRegion,
    pub scene: ChainRegion,
    pub hunter: HunterProfile,
    pub camera: CameraProfile,
    pub species: SpeciesProfile,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Meta {
    pub game: String,
    pub region: String,
    pub version: String,
    #[serde(deserialize_with = "de_u64")]
    pub title_id: u64,
    pub update_title_version: u16,
    #[serde(default)]
    pub fingerprint: Vec<FingerprintWindow>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FingerprintWindow {
    #[serde(deserialize_with = "de_u32")]
    pub addr: u32,
    pub len: u32,
    #[serde(deserialize_with = "de_u64")]
    pub xxh3: u64,
}

impl FingerprintWindow {
    pub fn is_placeholder(&self) -> bool {
        self.xxh3 == 0
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonsterList {
    pub base_candidates: Vec<BaseCandidate>,
    pub slots: u32,
    pub slot_stride: u32,
    #[serde(deserialize_with = "de_i32")]
    pub slot_offset: i32,
    pub chain: ChainSpec,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaseCandidate {
    #[serde(deserialize_with = "de_u32")]
    pub addr: u32,
    #[serde(deserialize_with = "de_u32_list")]
    pub expect: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonsterLayout {
    pub hp: FieldRef,
    pub max_hp: FieldRef,
    pub species: FieldRef,
    pub size: FieldRef,
    pub pos: FieldRef,
    pub poison: FieldRef,
    pub visible_flag: FieldRef,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChainRegion {
    pub chain: ChainSpec,
    #[serde(default, deserialize_with = "de_opt_u32")]
    pub addr: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HunterProfile {
    /// Static address of a pointer. One deref, then `pos.off`, is the feet vec3.
    #[serde(default, deserialize_with = "de_opt_u32")]
    pub base: Option<u32>,
    pub pos: FieldRef,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraProfile {
    pub mode: CameraMode,
    pub fov_unit: FovUnit,
    /// Static address of a pointer. One deref, then each field offset, is eye / target / fov.
    #[serde(default, deserialize_with = "de_opt_u32")]
    pub base: Option<u32>,
    #[serde(default)]
    pub eye: Option<FieldRef>,
    #[serde(default)]
    pub target: Option<FieldRef>,
    #[serde(default)]
    pub fov_y: Option<FieldRef>,
    #[serde(default)]
    pub matrix: Option<FieldRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CameraMode {
    Params,
    Matrix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FovUnit {
    Rad,
    Deg,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SpeciesProfile {
    pub default_anchor_height: f32,
    #[serde(default, flatten)]
    pub by_id: BTreeMap<String, SpeciesAnchor>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeciesAnchor {
    pub anchor_height: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChainSpec {
    Unresolved,
    Offsets(Vec<i32>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldRef {
    Unresolved,
    Relative(FieldSpec),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldSpec {
    pub off: i32,
    pub ty: FieldType,
    pub hidden_value: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum FieldType {
    #[serde(rename = "u8")]
    U8,
    #[serde(rename = "u16")]
    U16,
    #[serde(rename = "u32")]
    U32,
    #[serde(rename = "f32")]
    F32,
    #[serde(rename = "vec3")]
    Vec3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservedWindow {
    pub addr: u32,
    pub xxh3: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    /// Title id matches and every recorded fingerprint window matches.
    Exact,
    /// Title id matches, but `.text` hashes are still placeholders.
    Provisional,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Selection<'a> {
    pub profile: &'a Profile,
    pub kind: MatchKind,
}

impl Profile {
    pub fn from_toml_str(text: &str) -> Result<Self, ProfileError> {
        let profile: Self = toml::from_str(text)?;
        profile.validate()?;
        Ok(profile)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, ProfileError> {
        let text = fs::read_to_string(path)?;
        Self::from_toml_str(&text)
    }

    pub fn validate(&self) -> Result<(), ProfileError> {
        let mut errors = Vec::new();
        if self.meta.game.trim().is_empty() {
            errors.push("meta.game is empty".to_string());
        }
        if self.meta.region.trim().is_empty() {
            errors.push("meta.region is empty".to_string());
        }
        if self.meta.version.trim().is_empty() {
            errors.push("meta.version is empty".to_string());
        }
        if self.meta.title_id == 0 {
            errors.push("meta.title_id is 0".to_string());
        }
        if self.meta.update_title_version == 0 {
            errors.push("meta.update_title_version is 0".to_string());
        }
        if self.monster_list.base_candidates.is_empty() {
            errors.push("monster_list.base_candidates is empty".to_string());
        }
        for candidate in &self.monster_list.base_candidates {
            if candidate.expect.is_empty() {
                errors.push(format!(
                    "monster_list candidate 0x{:08X} has an empty expect list",
                    candidate.addr
                ));
            }
        }
        if !(1..=64).contains(&self.monster_list.slots) {
            errors.push("monster_list.slots must be between 1 and 64".to_string());
        }
        if self.monster_list.slot_stride == 0 {
            errors.push("monster_list.slot_stride is 0".to_string());
        }
        if let ChainSpec::Offsets(offsets) = &self.monster_list.chain {
            if offsets.is_empty() {
                errors.push("monster_list.chain is empty".to_string());
            }
        }
        if !is_positive(self.species.default_anchor_height) {
            errors.push("species.default_anchor_height must be positive".to_string());
        }
        for (id, anchor) in &self.species.by_id {
            if !is_positive(anchor.anchor_height) {
                errors.push(format!("species.{id}.anchor_height must be positive"));
            }
        }
        for window in &self.meta.fingerprint {
            if window.len == 0 || window.len > 65_536 {
                errors.push(format!(
                    "fingerprint at 0x{:08X} has invalid length {}",
                    window.addr, window.len
                ));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(ProfileError::Invalid(errors))
        }
    }

    pub fn unresolved_fields(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        push_field(&mut names, "monster.hp", &self.monster.hp);
        push_field(&mut names, "monster.max_hp", &self.monster.max_hp);
        push_field(&mut names, "monster.species", &self.monster.species);
        push_field(&mut names, "monster.size", &self.monster.size);
        push_field(&mut names, "monster.pos", &self.monster.pos);
        push_field(&mut names, "monster.poison", &self.monster.poison);
        push_field(
            &mut names,
            "monster.visible_flag",
            &self.monster.visible_flag,
        );
        push_chain(&mut names, "frame_counter", &self.frame_counter.chain);
        push_chain(&mut names, "scene", &self.scene.chain);
        push_field(&mut names, "hunter.pos", &self.hunter.pos);
        match self.camera.mode {
            CameraMode::Params => {
                push_opt(&mut names, "camera.eye", &self.camera.eye);
                push_opt(&mut names, "camera.target", &self.camera.target);
                push_opt(&mut names, "camera.fov_y", &self.camera.fov_y);
            }
            CameraMode::Matrix => {
                push_opt(&mut names, "camera.matrix", &self.camera.matrix);
            }
        }
        names
    }

    pub fn id(&self) -> String {
        format!(
            "{}-{}-{}",
            self.meta.game.to_ascii_lowercase(),
            self.meta.region.to_ascii_lowercase(),
            self.meta.version
        )
    }
}

pub fn load_dir(dir: impl AsRef<Path>) -> Result<Vec<Profile>, ProfileError> {
    let mut profiles = Vec::new();
    let mut paths: Vec<_> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    for path in paths {
        profiles.push(Profile::load(path)?);
    }
    Ok(profiles)
}

pub fn select<'a>(
    profiles: &'a [Profile],
    title_id: u64,
    observed: &[ObservedWindow],
) -> Option<Selection<'a>> {
    let mut provisional = None;
    for profile in profiles {
        if profile.meta.title_id != title_id {
            continue;
        }
        let recorded: Vec<_> = profile
            .meta
            .fingerprint
            .iter()
            .filter(|window| !window.is_placeholder())
            .collect();
        if recorded.is_empty() {
            if provisional.is_none() {
                provisional = Some(Selection {
                    profile,
                    kind: MatchKind::Provisional,
                });
            }
            continue;
        }
        let all_match = recorded.iter().all(|window| {
            observed
                .iter()
                .any(|seen| seen.addr == window.addr && seen.xxh3 == window.xxh3)
        });
        if all_match {
            return Some(Selection {
                profile,
                kind: MatchKind::Exact,
            });
        }
    }
    provisional
}

fn is_positive(value: f32) -> bool {
    value.is_finite() && value > 0.0
}

fn push_field(names: &mut Vec<&'static str>, name: &'static str, field: &FieldRef) {
    if matches!(field, FieldRef::Unresolved) {
        names.push(name);
    }
}

fn push_opt(names: &mut Vec<&'static str>, name: &'static str, field: &Option<FieldRef>) {
    if matches!(field, Some(FieldRef::Unresolved) | None) {
        names.push(name);
    }
}

fn push_chain(names: &mut Vec<&'static str>, name: &'static str, chain: &ChainSpec) {
    if matches!(chain, ChainSpec::Unresolved) {
        names.push(name);
    }
}

impl<'de> Deserialize<'de> for ChainSpec {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ChainVisitor;
        impl<'de> Visitor<'de> for ChainVisitor {
            type Value = ChainSpec;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("\"TBD\" or a list of offsets")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<ChainSpec, E> {
                if value.eq_ignore_ascii_case("TBD") {
                    Ok(ChainSpec::Unresolved)
                } else {
                    Err(E::custom(format!(
                        "expected \"TBD\" or a list of offsets, got '{value}'"
                    )))
                }
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<ChainSpec, A::Error> {
                let mut offsets = Vec::new();
                while let Some(offset) = seq.next_element::<FlexI32>()? {
                    offsets.push(offset.0);
                }
                if offsets.is_empty() {
                    return Err(de::Error::custom("offset chain is empty"));
                }
                Ok(ChainSpec::Offsets(offsets))
            }
        }
        deserializer.deserialize_any(ChainVisitor)
    }
}

impl<'de> Deserialize<'de> for FieldRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct FieldVisitor;
        impl<'de> Visitor<'de> for FieldVisitor {
            type Value = FieldRef;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("\"TBD\" or a { off, ty } table")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<FieldRef, E> {
                if value.eq_ignore_ascii_case("TBD") {
                    Ok(FieldRef::Unresolved)
                } else {
                    Err(E::custom(format!("expected \"TBD\", got '{value}'")))
                }
            }

            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<FieldRef, M::Error> {
                let spec = FieldSpecDe::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(FieldRef::Relative(FieldSpec {
                    off: spec.off,
                    ty: spec.ty,
                    hidden_value: spec.hidden_value,
                }))
            }
        }
        deserializer.deserialize_any(FieldVisitor)
    }
}

#[derive(Deserialize)]
struct FieldSpecDe {
    #[serde(deserialize_with = "de_i32")]
    off: i32,
    ty: FieldType,
    #[serde(default, deserialize_with = "de_opt_u32")]
    hidden_value: Option<u32>,
}

struct FlexI32(i32);

impl<'de> Deserialize<'de> for FlexI32 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        de_i32(deserializer).map(FlexI32)
    }
}

fn de_u32<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    struct V;
    impl Visitor<'_> for V {
        type Value = u32;
        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an integer or hex string")
        }
        fn visit_u64<E: de::Error>(self, value: u64) -> Result<u32, E> {
            u32::try_from(value).map_err(E::custom)
        }
        fn visit_i64<E: de::Error>(self, value: i64) -> Result<u32, E> {
            u32::try_from(value).map_err(E::custom)
        }
        fn visit_str<E: de::Error>(self, value: &str) -> Result<u32, E> {
            parse_u32(value).map_err(E::custom)
        }
    }
    deserializer.deserialize_any(V)
}

fn de_u64<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    struct V;
    impl Visitor<'_> for V {
        type Value = u64;
        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an integer or hex string")
        }
        fn visit_u64<E: de::Error>(self, value: u64) -> Result<u64, E> {
            Ok(value)
        }
        fn visit_i64<E: de::Error>(self, value: i64) -> Result<u64, E> {
            u64::try_from(value).map_err(E::custom)
        }
        fn visit_str<E: de::Error>(self, value: &str) -> Result<u64, E> {
            parse_u64(value).map_err(E::custom)
        }
    }
    deserializer.deserialize_any(V)
}

fn de_i32<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i32, D::Error> {
    struct V;
    impl Visitor<'_> for V {
        type Value = i32;
        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an integer or hex string")
        }
        fn visit_i64<E: de::Error>(self, value: i64) -> Result<i32, E> {
            i32::try_from(value).map_err(E::custom)
        }
        fn visit_u64<E: de::Error>(self, value: u64) -> Result<i32, E> {
            i32::try_from(value).map_err(E::custom)
        }
        fn visit_str<E: de::Error>(self, value: &str) -> Result<i32, E> {
            parse_i32(value).map_err(E::custom)
        }
    }
    deserializer.deserialize_any(V)
}

fn de_opt_u32<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u32>, D::Error> {
    de_u32(deserializer).map(Some)
}

fn de_u32_list<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u32>, D::Error> {
    struct V;
    impl<'de> Visitor<'de> for V {
        type Value = Vec<u32>;
        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a list of integers or hex strings")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<u32>, A::Error> {
            let mut values = Vec::new();
            while let Some(value) = seq.next_element::<FlexU32>()? {
                values.push(value.0);
            }
            Ok(values)
        }
    }
    deserializer.deserialize_seq(V)
}

struct FlexU32(u32);

impl<'de> Deserialize<'de> for FlexU32 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        de_u32(deserializer).map(FlexU32)
    }
}

fn parse_u32(input: &str) -> Result<u32, String> {
    let value = parse_u64(input)?;
    u32::try_from(value).map_err(|_| format!("'{input}' does not fit in u32"))
}

fn parse_u64(input: &str) -> Result<u64, String> {
    let text = input.trim();
    let hex = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X"));
    if let Some(hex) = hex {
        u64::from_str_radix(hex, 16).map_err(|err| format!("invalid hex '{input}': {err}"))
    } else {
        text.parse::<u64>()
            .map_err(|err| format!("invalid integer '{input}': {err}"))
    }
}

fn parse_i32(input: &str) -> Result<i32, String> {
    let text = input.trim();
    let negative = text.starts_with('-');
    let text = text.strip_prefix('-').unwrap_or(text);
    let value = parse_u32(text)?;
    if value > i32::MAX as u32 {
        return Err(format!("'{input}' does not fit in i32"));
    }
    if negative {
        Ok(-(value as i32))
    } else {
        Ok(value as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> &'static str {
        include_str!("../../../profiles/mhxx-jp-v1.4-es.toml")
    }

    #[test]
    fn loads_the_mhxx_spanish_profile() {
        let profile = Profile::from_toml_str(sample()).unwrap();
        assert_eq!(profile.meta.title_id, MHXX_JP_TITLE_ID);
        assert_eq!(profile.meta.update_title_version, 4224);
        assert_eq!(profile.id(), "mhxx-jp-1.4-es");
        assert_eq!(profile.monster_list.base_candidates.len(), 3);
        assert_eq!(
            profile.monster_list.chain,
            ChainSpec::Offsets(vec![0x10A8, 0x360])
        );
        match &profile.monster.size {
            FieldRef::Relative(spec) => {
                assert_eq!(spec.off, -432);
                assert_eq!(spec.ty, FieldType::F32);
            }
            FieldRef::Unresolved => panic!("size should be resolved"),
        }
        match &profile.monster.visible_flag {
            FieldRef::Relative(spec) => assert_eq!(spec.hidden_value, Some(0x7)),
            FieldRef::Unresolved => panic!("visible flag should be resolved"),
        }
        match &profile.monster.pos {
            FieldRef::Relative(spec) => {
                assert_eq!(spec.off, -800);
                assert_eq!(spec.ty, FieldType::Vec3);
            }
            FieldRef::Unresolved => panic!("pos should be resolved"),
        }
        let third = &profile.monster_list.base_candidates[2];
        assert!(third.expect.contains(&0x082C_E730));
        let pending = profile.unresolved_fields();
        assert!(!pending.contains(&"monster.pos"));
        assert!(pending.contains(&"frame_counter"));
        assert!(pending.contains(&"scene"));
        assert_eq!(profile.hunter.base, Some(0x0814_E620));
        match &profile.hunter.pos {
            FieldRef::Relative(spec) => {
                assert_eq!(spec.off, 64);
                assert_eq!(spec.ty, FieldType::Vec3);
            }
            FieldRef::Unresolved => panic!("hunter.pos should be resolved"),
        }
        assert_eq!(profile.camera.base, Some(0x0814_CACC));
        assert_eq!(profile.camera.fov_unit, FovUnit::Deg);
        match profile.camera.eye.as_ref() {
            Some(FieldRef::Relative(spec)) => {
                assert_eq!(spec.off, 64);
                assert_eq!(spec.ty, FieldType::Vec3);
            }
            other => panic!("camera.eye should be resolved, got {other:?}"),
        }
        match profile.camera.target.as_ref() {
            Some(FieldRef::Relative(spec)) => assert_eq!(spec.off, 96),
            other => panic!("camera.target should be resolved, got {other:?}"),
        }
        match profile.camera.fov_y.as_ref() {
            Some(FieldRef::Relative(spec)) => {
                assert_eq!(spec.off, 60);
                assert_eq!(spec.ty, FieldType::F32);
            }
            other => panic!("camera.fov_y should be resolved, got {other:?}"),
        }
        assert!(!pending.contains(&"hunter.pos"));
        assert!(!pending.contains(&"camera.eye"));
        assert!(!pending.contains(&"monster.hp"));
    }

    #[test]
    fn rejects_an_empty_slot_count() {
        let mut text = sample().to_string();
        text = text.replace("slots = 16", "slots = 0");
        let err = Profile::from_toml_str(&text).unwrap_err();
        assert!(err.to_string().contains("slots"));
    }

    #[test]
    fn selects_provisionally_until_fingerprints_are_filled() {
        let profile = Profile::from_toml_str(sample()).unwrap();
        let profiles = [profile];
        let chosen = select(&profiles, MHXX_JP_TITLE_ID, &[]).unwrap();
        assert_eq!(chosen.kind, MatchKind::Provisional);
        assert!(select(&[], 0x1234, &[]).is_none());
    }

    #[test]
    fn exact_match_requires_every_recorded_window() {
        let mut profile = Profile::from_toml_str(sample()).unwrap();
        profile.meta.fingerprint[0].xxh3 = 0x1111;
        profile.meta.fingerprint[1].xxh3 = 0x2222;
        let observed = [
            ObservedWindow {
                addr: 0x0010_0000,
                xxh3: 0x1111,
            },
            ObservedWindow {
                addr: 0x0014_0000,
                xxh3: 0x2222,
            },
        ];
        let profiles = [profile];
        assert_eq!(
            select(&profiles, MHXX_JP_TITLE_ID, &observed).unwrap().kind,
            MatchKind::Exact
        );
        let wrong = [ObservedWindow {
            addr: 0x0010_0000,
            xxh3: 0x1111,
        }];
        assert!(select(&profiles, MHXX_JP_TITLE_ID, &wrong).is_none());
    }
}
