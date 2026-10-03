//! What the menu-bar item says. The overlay does not guess a version it has not read.

#![forbid(unsafe_code)]

use crate::session::{PHASE_DOWN, PHASE_LIVE, PHASE_UNSUPPORTED};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    WaitingForAzahar,
    RpcDisabled,
    Unsupported,
    Active,
}

pub fn link_state(phase: u8, window_onscreen: bool) -> LinkState {
    match phase {
        PHASE_DOWN => LinkState::RpcDisabled,
        PHASE_UNSUPPORTED => LinkState::Unsupported,
        PHASE_LIVE if window_onscreen => LinkState::Active,
        _ => LinkState::WaitingForAzahar,
    }
}

pub fn status_title(state: LinkState, version: &str) -> String {
    match state {
        LinkState::WaitingForAzahar => "mhdn: Waiting for Azahar".to_string(),
        LinkState::RpcDisabled => "mhdn: RPC off".to_string(),
        LinkState::Unsupported => format!("mhdn: Unsupported game ({version})"),
        LinkState::Active => "mhdn: Active".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::PHASE_WAITING;

    #[test]
    fn the_four_states_have_stable_titles() {
        assert_eq!(
            link_state(PHASE_WAITING, false),
            LinkState::WaitingForAzahar
        );
        assert_eq!(link_state(PHASE_LIVE, false), LinkState::WaitingForAzahar);
        assert_eq!(link_state(PHASE_DOWN, true), LinkState::RpcDisabled);
        assert_eq!(link_state(PHASE_UNSUPPORTED, true), LinkState::Unsupported);
        assert_eq!(link_state(PHASE_LIVE, true), LinkState::Active);
        assert_eq!(
            status_title(LinkState::Unsupported, "v1.4-es"),
            "mhdn: Unsupported game (v1.4-es)"
        );
        assert_eq!(status_title(LinkState::Active, "v1.4-es"), "mhdn: Active");
        assert_eq!(
            status_title(LinkState::WaitingForAzahar, "v1.4-es"),
            "mhdn: Waiting for Azahar"
        );
        assert_eq!(
            status_title(LinkState::RpcDisabled, "v1.4-es"),
            "mhdn: RPC off"
        );
    }
}
