//! What the glow shows, and the numbers `quire-agent-v1` carries for it. The protocol's enums
//! (`protocols/quire-agent-v1.xml`) and these tables are the same values; a test in `tests/it`
//! compares them with the generated bindings.

use crate::lease::{RevokeReason, Suspend};
use cua_action::RunMode;
use cua_action::{Rect, WindowSpace};
use serde::{Deserialize, Serialize};

/// What the run looks like from outside: the compositor (fork) or the shell (stock) draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum GlowState {
    /// Nothing.
    Off,
    /// Looking or thinking.
    Working,
    /// Acting here, at this window-local region.
    Acting {
        /// Where the action lands.
        at: Rect<WindowSpace>,
    },
    /// Needs the person.
    WaitingForYou,
}

/// A protocol enum value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WireCode(pub u32);

impl GlowState {
    /// The protocol's `glow_state` value.
    pub fn code(&self) -> WireCode {
        WireCode(match self {
            GlowState::Off => 0,
            GlowState::Working => 1,
            GlowState::Acting { .. } => 2,
            GlowState::WaitingForYou => 3,
        })
    }
}

impl RevokeReason {
    /// The protocol's `revoke_reason` value.
    pub fn code(self) -> WireCode {
        WireCode(match self {
            RevokeReason::KillChord => 0,
            RevokeReason::WindowClosed => 1,
            RevokeReason::SpaceChanged => 2,
            RevokeReason::Compositor => 3,
        })
    }

    /// The reason a protocol value names, if it names one.
    pub fn from_code(code: WireCode) -> Option<Self> {
        [
            RevokeReason::KillChord,
            RevokeReason::WindowClosed,
            RevokeReason::SpaceChanged,
            RevokeReason::Compositor,
        ]
        .into_iter()
        .find(|reason| reason.code() == code)
    }
}

impl Suspend {
    /// The protocol's `suspend_reason` value.
    pub fn code(self) -> WireCode {
        WireCode(match self {
            Suspend::TakeOver => 0,
            Suspend::Consent => 1,
            Suspend::Paused => 2,
        })
    }
}

/// The protocol's `lease_mode` value for a run mode.
pub fn mode_code(mode: RunMode) -> WireCode {
    WireCode(match mode {
        RunMode::InPlace => 0,
        RunMode::AgentWorkspace => 1,
        RunMode::NestedSession => 2,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cua_action::{Coord, Point, Size};

    fn region() -> Rect<WindowSpace> {
        Rect {
            origin: Point::new(Coord(10), Coord(20)),
            size: Size::new(Coord(30), Coord(40)),
        }
    }

    #[test]
    fn glow_state_and_revoke_reason_wire_values() {
        let glow = [
            (GlowState::Off, 0),
            (GlowState::Working, 1),
            (GlowState::Acting { at: region() }, 2),
            (GlowState::WaitingForYou, 3),
        ];
        for (state, want) in glow {
            assert_eq!(state.code(), WireCode(want), "{state:?}");
        }
        let revoke = [
            (RevokeReason::KillChord, 0),
            (RevokeReason::WindowClosed, 1),
            (RevokeReason::SpaceChanged, 2),
            (RevokeReason::Compositor, 3),
        ];
        for (reason, want) in revoke {
            assert_eq!(reason.code(), WireCode(want), "{reason:?}");
            assert_eq!(RevokeReason::from_code(WireCode(want)), Some(reason));
        }
        assert_eq!(RevokeReason::from_code(WireCode(9)), None);
        let suspend = [
            (Suspend::TakeOver, 0),
            (Suspend::Consent, 1),
            (Suspend::Paused, 2),
        ];
        for (why, want) in suspend {
            assert_eq!(why.code(), WireCode(want), "{why:?}");
        }
        let modes = [
            (RunMode::InPlace, 0),
            (RunMode::AgentWorkspace, 1),
            (RunMode::NestedSession, 2),
        ];
        for (mode, want) in modes {
            assert_eq!(mode_code(mode), WireCode(want), "{mode:?}");
        }
    }

    #[test]
    fn glow_state_round_trips_with_its_region() {
        let state = GlowState::Acting { at: region() };
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"acting","v":{"at":{"origin":{"x":10,"y":20},"size":{"w":30,"h":40}}}}"#
        );
        assert_eq!(serde_json::from_str::<GlowState>(&json).unwrap(), state);
    }
}
