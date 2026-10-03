//! What is hidden from the model, and what was actually hidden.

use crate::pixels::{BYTES_PER_PIXEL, PixelBuf};
use cua_action::{Coord, Rect, Scale120, Size, WindowSpace};
use cua_desktop::MaskWhere;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

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

/// The black of a masked pixel: opaque, whichever of the four formats.
const BLACK: [u8; BYTES_PER_PIXEL] = [0, 0, 0, 255];

/// A device-pixel edge for a logical one: `round(logical * scale / 120)`.
fn device_edge(logical: u32, scale: Scale120) -> u64 {
    (u64::from(logical) * u64::from(scale.0) + 60) / 120
}

/// Blacks the exact device pixels of `regions` (the rounding is `round(logical * scale / 120)`
/// per edge, so a 2-lpx region at 1.5 becomes exactly 3 device pixels) and returns a new buffer
/// and the regions as applied, clamped to the buffer's logical size. A region wholly outside the
/// buffer is not applied and not reported. Pure.
pub fn apply_masks(
    buf: PixelBuf,
    scale: Scale120,
    regions: &[MaskRegion],
) -> (PixelBuf, Vec<MaskRegion>) {
    let stride = usize::try_from(buf.stride).unwrap_or(usize::MAX);
    let logical_w =
        (u64::from(buf.size.w) * 120 + u64::from(scale.0) / 2) / u64::from(scale.0.max(1));
    let logical_h =
        (u64::from(buf.size.h) * 120 + u64::from(scale.0) / 2) / u64::from(scale.0.max(1));
    let mut data = buf.data.to_vec();
    let mut applied = Vec::new();
    for region in regions {
        let (origin, size) = (region.rect.origin, region.rect.size);
        let x1 = (u64::from(origin.x.0) + u64::from(size.w.0)).min(logical_w);
        let y1 = (u64::from(origin.y.0) + u64::from(size.h.0)).min(logical_h);
        let (x0, y0) = (u64::from(origin.x.0), u64::from(origin.y.0));
        if x0 >= x1 || y0 >= y1 {
            continue;
        }
        let to_u32 = |n: u64| u32::try_from(n).unwrap_or(u32::MAX);
        applied.push(MaskRegion {
            rect: Rect {
                origin,
                size: Size::new(Coord(to_u32(x1 - x0)), Coord(to_u32(y1 - y0))),
            },
            why: region.why,
        });
        let dev =
            |logical: u64, limit: u32| device_edge(to_u32(logical), scale).min(u64::from(limit));
        let (dx0, dx1) = (dev(x0, buf.size.w), dev(x1, buf.size.w));
        let (dy0, dy1) = (dev(y0, buf.size.h), dev(y1, buf.size.h));
        for y in dy0..dy1 {
            let row = usize::try_from(y)
                .unwrap_or(usize::MAX)
                .saturating_mul(stride);
            for x in dx0..dx1 {
                let at =
                    row.saturating_add(usize::try_from(x).unwrap_or(usize::MAX) * BYTES_PER_PIXEL);
                if let Some(pixel) = data.get_mut(at..at + BYTES_PER_PIXEL) {
                    pixel.copy_from_slice(&BLACK);
                }
            }
        }
    }
    (
        PixelBuf {
            data: Arc::from(data),
            ..buf
        },
        applied,
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
