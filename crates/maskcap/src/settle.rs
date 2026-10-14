//! Deciding when the screen has stopped changing after an action.
//!
//! A stock compositor does not gate frames on damage: every captured frame reports full damage
//! within about 10 ms, even for a static window. So on stock the [`DamageAt`] events are made by
//! cuad's pixel diff ([`DamageSource::PixelDiff`]); on the fork they come from the compositor.

use crate::pixels::{BYTES_PER_PIXEL, PixelBuf};
use cua_action::{Coord, Point, Rect, Size, WindowSpace};
use cua_desktop::{Ms, Stamp};
use serde::{Deserialize, Serialize};

/// Where a damage event came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DamageSource {
    /// The compositor reported it.
    Compositor,
    /// cuad compared two frames.
    PixelDiff,
}

/// The screen changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DamageAt {
    /// When the changed frame arrived.
    pub at: Stamp,
    /// Where, window-local.
    pub region: Rect<WindowSpace>,
    /// Who found out.
    pub source: DamageSource,
}

/// Whether to keep waiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Settle {
    /// Nothing changed for the quiet period.
    Quiet,
    /// Wait; look again at `wake`.
    Waiting {
        /// When.
        wake: Stamp,
    },
    /// The timeout passed with the screen still changing.
    TimedOut,
}

/// Decides from the damage since the action. The screen is quiet once nothing has changed for
/// `quiet`, counted from the later of the action (`since`) and the newest damage event, so a
/// screen that has not changed yet is not settled the moment the action ends. Otherwise it has
/// timed out when `timeout` has passed since `since`, and else it is waiting, with `wake` at
/// the earlier of the end of the quiet period and the timeout. Events before `since` are not
/// the action's and are ignored. Pure.
pub fn settle(events: &[DamageAt], quiet: Ms, timeout: Ms, since: Stamp, now: Stamp) -> Settle {
    let last = events
        .iter()
        .map(|e| e.at)
        .filter(|at| *at >= since)
        .max()
        .map_or(since, |newest| newest.max(since));
    let quiet_at = last.0.saturating_add(u64::from(quiet.0));
    let timeout_at = since.0.saturating_add(u64::from(timeout.0));
    if now.0 >= quiet_at {
        Settle::Quiet
    } else if now.0 >= timeout_at {
        Settle::TimedOut
    } else {
        Settle::Waiting {
            wake: Stamp(quiet_at.min(timeout_at)),
        }
    }
}

/// Row `y` of `buf`, `row_bytes` long, when the data reaches that far.
fn row_of(buf: &PixelBuf, y: usize, row_bytes: usize) -> Option<&[u8]> {
    let start = y.checked_mul(usize::try_from(buf.stride).ok()?)?;
    buf.data.get(start..start.checked_add(row_bytes)?)
}

/// The bounding box of the pixels that differ between two frames, in the buffers' own pixels,
/// or `None` when they are equal. Stock captures at logical resolution (`Frame.scale` is 120
/// there), so these are window-local logical pixels; a caller with another scale divides by it.
/// Two buffers that do not have the same format and size differ everywhere: the whole of `next`.
/// Stride padding is not compared. Pure.
pub fn pixel_diff(prev: &PixelBuf, next: &PixelBuf) -> Option<Rect<WindowSpace>> {
    let (w, h) = (next.size.w, next.size.h);
    let whole = Rect {
        origin: Point::new(Coord(0), Coord(0)),
        size: Size::new(Coord(w), Coord(h)),
    };
    if prev.format != next.format || prev.size != next.size {
        return Some(whole);
    }
    let row_bytes = usize::try_from(w).ok()?.checked_mul(BYTES_PER_PIXEL)?;
    let mut x_range: Option<(usize, usize)> = None;
    let mut y_range: Option<(usize, usize)> = None;
    for y in 0..usize::try_from(h).ok()? {
        let (Some(a), Some(b)) = (row_of(prev, y, row_bytes), row_of(next, y, row_bytes)) else {
            // A buffer too short for its own size: nothing is known to match.
            return Some(whole);
        };
        if a == b {
            continue;
        }
        let first = a
            .chunks(BYTES_PER_PIXEL)
            .zip(b.chunks(BYTES_PER_PIXEL))
            .position(|(p, q)| p != q)?;
        let last = a
            .chunks(BYTES_PER_PIXEL)
            .zip(b.chunks(BYTES_PER_PIXEL))
            .rposition(|(p, q)| p != q)?;
        x_range = Some(x_range.map_or((first, last), |(lo, hi)| (lo.min(first), hi.max(last))));
        y_range = Some(y_range.map_or((y, y), |(lo, _)| (lo, y)));
    }
    let ((x0, x1), (y0, y1)) = (x_range?, y_range?);
    let to_coord = |n: usize| Coord(u32::try_from(n).unwrap_or(u32::MAX));
    Some(Rect {
        origin: Point::new(to_coord(x0), to_coord(y0)),
        size: Size::new(to_coord(x1 - x0 + 1), to_coord(y1 - y0 + 1)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settle_and_damage_pin_their_json() {
        let at = DamageAt {
            at: Stamp(900),
            region: Rect {
                origin: Point::new(Coord(0), Coord(0)),
                size: Size::new(Coord(10), Coord(20)),
            },
            source: DamageSource::PixelDiff,
        };
        let json = serde_json::to_string(&at).unwrap();
        assert_eq!(
            json,
            r#"{"at":900,"region":{"origin":{"x":0,"y":0},"size":{"w":10,"h":20}},"source":"pixel_diff"}"#
        );
        assert_eq!(serde_json::from_str::<DamageAt>(&json).unwrap(), at);
        let table = [
            (Settle::Quiet, r#"{"kind":"quiet"}"#),
            (
                Settle::Waiting { wake: Stamp(5) },
                r#"{"kind":"waiting","v":{"wake":5}}"#,
            ),
            (Settle::TimedOut, r#"{"kind":"timed_out"}"#),
        ];
        for (settle, want) in table {
            assert_eq!(serde_json::to_string(&settle).unwrap(), want);
            assert_eq!(serde_json::from_str::<Settle>(want).unwrap(), settle);
        }
    }
}
