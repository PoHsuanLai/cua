//! What is hidden from the model, and what was actually hidden.

use crate::pixels::PixelBuf;
use cua_action::{Rect, Scale120, WindowSpace};
use cua_desktop::MaskWhere;
use serde::{Deserialize, Serialize};

/// Why a region is masked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaskReason {
    /// A password field (text-input purpose, or an accessibility `PasswordText` role).
    PasswordField,
    /// The app declared the region sensitive (`quire_sensitive_v1`).
    SurfaceDeclared,
    /// An accessibility node marked secret.
    A11ySecret,
    /// A window class whose content is never shown (password managers, banking).
    SensitiveWindow,
    /// Content from another Space.
    OtherSpace,
    /// An overlay that is not part of the leased window.
    OverlayOutsideLease,
}

impl MaskReason {
    /// The `mask_reason` value of `quire-agent-v1`.
    pub fn code(self) -> u32 {
        match self {
            MaskReason::PasswordField => 0,
            MaskReason::SurfaceDeclared => 1,
            MaskReason::A11ySecret => 2,
            MaskReason::SensitiveWindow => 3,
            MaskReason::OtherSpace => 4,
            MaskReason::OverlayOutsideLease => 5,
        }
    }
}

/// A window-local region and why it is hidden. A hint going in (from `a11y_tree::secret_rects`,
/// the sensitive-window rules and the text-input heuristic); in a [`MaskReport`], a region as
/// applied, clamped to the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaskRegion {
    /// Window-local logical pixels.
    pub rect: Rect<WindowSpace>,
    /// Why.
    pub why: MaskReason,
}

/// Whether the whole frame was blacked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum WholeMask {
    /// Only regions, if any.
    None,
    /// Every pixel, for this reason.
    Blacked(MaskReason),
}

/// What was masked in one frame, and by whom. Pixels never leave cuad unmasked, whichever side
/// did it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaskReport {
    /// The compositor (fork) or cuad (stock).
    pub applied_by: MaskWhere,
    /// Whole-frame mask, if any.
    pub whole: WholeMask,
    /// The regions applied.
    pub regions: Vec<MaskRegion>,
}

/// Blacks the exact device pixels of `regions` (the rounding is `round(logical * scale / 120)`
/// per edge, so a 2-lpx region at 1.5 becomes exactly 3 device pixels) and returns a new buffer
/// and the regions as applied, clamped to the buffer. Pure.
pub fn apply_masks(
    buf: PixelBuf,
    scale: Scale120,
    regions: &[MaskRegion],
) -> (PixelBuf, Vec<MaskRegion>) {
    let _ = (buf, scale, regions);
    todo!(
        "black the device pixels of each region, clamp, return a new buffer (apply_masks_blacks_exact_device_pixels_at_scale_150)"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_reason_codes_follow_the_protocol_order() {
        let table = [
            (MaskReason::PasswordField, 0),
            (MaskReason::SurfaceDeclared, 1),
            (MaskReason::A11ySecret, 2),
            (MaskReason::SensitiveWindow, 3),
            (MaskReason::OtherSpace, 4),
            (MaskReason::OverlayOutsideLease, 5),
        ];
        for (reason, code) in table {
            assert_eq!(reason.code(), code, "{reason:?}");
        }
    }

    #[test]
    fn a_report_pins_its_json() {
        let report = MaskReport {
            applied_by: MaskWhere::Cuad,
            whole: WholeMask::None,
            regions: vec![],
        };
        let json = serde_json::to_string(&report).unwrap();
        assert_eq!(
            json,
            r#"{"applied_by":"cuad","whole":{"kind":"none"},"regions":[]}"#
        );
        assert_eq!(serde_json::from_str::<MaskReport>(&json).unwrap(), report);
        let whole = WholeMask::Blacked(MaskReason::SensitiveWindow);
        assert_eq!(
            serde_json::to_string(&whole).unwrap(),
            r#"{"kind":"blacked","v":"sensitive_window"}"#
        );
    }
}
