//! What a backend can do. The values for the backends are a table (`BackendCaps::of`) so
//! `mode_support` and the tests ask the table rather than a copy of it.

use crate::lease::SeatIsolation;
use serde::{Deserialize, Serialize};

/// Which compositor cuad is talking to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    /// Stock KWin: the nested-session host and the portability reference.
    KwinStock,
}

/// Who masks a frame before pixels reach the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaskWhere {
    /// The compositor (fork): the buffer arrives masked.
    Compositor,
    /// cuad (stock): `maskcap::apply_masks` runs on the unmasked frame.
    Cuad,
}

/// Who guarantees that a confirmation cannot be answered by the agent's own input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsentGuard {
    /// The compositor accepts physical input only on a consent surface (fork).
    CompositorOrigin,
    /// cuad stops emulating while the confirmation shows (stock). A third-party prompt
    /// overlapping the window remains a stated residual risk.
    CuadSuspendsInput,
}

/// Whether a window on another workspace, or minimized, keeps rendering for capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HiddenCapture {
    /// Frames keep arriving and the content keeps updating (measured on a stock compositor).
    Renders,
    /// The compositor stops rendering it.
    Withheld,
}

/// Who draws the glow and the agent's cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlowDraw {
    /// The compositor, from lease state.
    Compositor,
    /// The shell's orb only.
    ShellOnly,
}

/// What one backend can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BackendCaps {
    /// Which backend.
    pub kind: BackendKind,
    /// Whether the agent has a seat of its own.
    pub seat: SeatIsolation,
    /// Who masks.
    pub masking: MaskWhere,
    /// Who guards confirmations.
    pub consent: ConsentGuard,
    /// Whether hidden windows keep rendering.
    pub hidden: HiddenCapture,
    /// Who draws the glow.
    pub glow: GlowDraw,
}

impl BackendCaps {
    /// The capabilities of `kind`. KWin's row is the portability reference (shared seat,
    /// unmasked capture, hidden windows render) and is unverified until Spike B.
    pub fn of(kind: BackendKind) -> BackendCaps {
        match kind {
            BackendKind::KwinStock => BackendCaps {
                kind,
                seat: SeatIsolation::SharedSeat,
                masking: MaskWhere::Cuad,
                consent: ConsentGuard::CuadSuspendsInput,
                hidden: HiddenCapture::Renders,
                glow: GlowDraw::ShellOnly,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kwin_backend_is_a_shared_seat_masked_by_cuad_and_guarded_by_suspending_input() {
        let kind = BackendKind::KwinStock;
        let caps = BackendCaps::of(kind);
        assert_eq!(caps.kind, kind);
        assert_eq!(caps.seat, SeatIsolation::SharedSeat);
        assert_eq!(caps.masking, MaskWhere::Cuad);
        assert_eq!(caps.consent, ConsentGuard::CuadSuspendsInput);
        assert_eq!(caps.hidden, HiddenCapture::Renders);
        assert_eq!(caps.glow, GlowDraw::ShellOnly);
    }
}
