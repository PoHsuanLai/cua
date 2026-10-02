//! Deciding when the screen has stopped changing after an action.
//!
//! Stock cosmic-comp does not gate frames on damage: every captured frame reports full damage
//! within about 10 ms, even for a static window. So on stock the [`DamageAt`] events are made by
//! cuad's pixel diff ([`DamageSource::PixelDiff`]); on the fork they come from the compositor.

use crate::pixels::PixelBuf;
use cua_action::{Rect, WindowSpace};
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

/// Decides from the damage since the action. Quiet when no event is newer than `quiet` before
/// `now`; timed out when `timeout` has passed since `since`; otherwise waiting until the quiet
/// period would end. Pure.
pub fn settle(events: &[DamageAt], quiet: Ms, timeout: Ms, since: Stamp, now: Stamp) -> Settle {
    let _ = (events, quiet, timeout, since, now);
    todo!("quiet, waiting and timed-out rows (settle_quiet_waiting_timeout_table)")
}

/// The bounding box of the pixels that differ between two frames of the same size, in
/// window-local logical pixels, or `None` when they are equal. Pure.
pub fn pixel_diff(prev: &PixelBuf, next: &PixelBuf) -> Option<Rect<WindowSpace>> {
    let _ = (prev, next);
    todo!("compare rows, bound the changed pixels, divide by the capture scale")
}

#[cfg(test)]
mod tests {
    use super::*;
    use cua_action::{Coord, Point, Size};

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
