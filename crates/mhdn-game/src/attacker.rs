//! Who dealt damage. Identification is filled in by a later phase; until then most hits stay [`Attacker::Unknown`].

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Attacker {
    You,
    YourFelyne,
    OtherHunter,
    OtherFelyne,
    Other,
    #[default]
    Unknown,
}

impl Attacker {
    pub const ALL: [Self; 6] = [
        Self::You,
        Self::YourFelyne,
        Self::OtherHunter,
        Self::OtherFelyne,
        Self::Other,
        Self::Unknown,
    ];

    pub fn index(self) -> usize {
        self.index_const()
    }

    const fn index_const(self) -> usize {
        match self {
            Self::You => 0,
            Self::YourFelyne => 1,
            Self::OtherHunter => 2,
            Self::OtherFelyne => 3,
            Self::Other => 4,
            Self::Unknown => 5,
        }
    }
}

/// Attackers counted until real attribution exists. Change this single list when defaults move.
pub const DEFAULT_ATTACKER_FILTER_ATTACKERS: &[Attacker] =
    &[Attacker::You, Attacker::YourFelyne, Attacker::Unknown];

/// Which attackers contribute to meter totals and corner text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttackerFilter {
    mask: u8,
}

impl Default for AttackerFilter {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl AttackerFilter {
    /// Default filter: you, your Palico, and unknown hits (until attribution exists).
    pub const DEFAULT: Self = Self::from_slice(DEFAULT_ATTACKER_FILTER_ATTACKERS);

    pub const fn from_slice(allowed: &[Attacker]) -> Self {
        let mut mask = 0u8;
        let mut i = 0;
        while i < allowed.len() {
            mask |= 1 << allowed[i].index_const();
            i += 1;
        }
        Self { mask }
    }

    pub fn allows(self, attacker: Attacker) -> bool {
        (self.mask & (1 << attacker.index())) != 0
    }

    pub fn with(mut self, attacker: Attacker, allowed: bool) -> Self {
        let bit = 1 << attacker.index();
        if allowed {
            self.mask |= bit;
        } else {
            self.mask &= !bit;
        }
        self
    }

    pub fn only_you_and_felyne() -> Self {
        Self::from_slice(&[Attacker::You, Attacker::YourFelyne])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_filter_matches_documented_attackers() {
        let filter = AttackerFilter::default();
        assert!(filter.allows(Attacker::You));
        assert!(filter.allows(Attacker::YourFelyne));
        assert!(filter.allows(Attacker::Unknown));
        assert!(!filter.allows(Attacker::OtherHunter));
        assert!(!filter.allows(Attacker::OtherFelyne));
        assert!(!filter.allows(Attacker::Other));
    }

    #[test]
    fn with_toggles_one_attacker() {
        let filter = AttackerFilter::default().with(Attacker::YourFelyne, false);
        assert!(!filter.allows(Attacker::YourFelyne));
        assert!(filter.allows(Attacker::You));
    }
}
