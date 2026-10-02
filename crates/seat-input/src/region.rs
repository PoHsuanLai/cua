//! Window-local points to the EI region, and back. Pure.
//!
//! On the fork the lease's EI region is the window, so the map is the identity. On stock the
//! region is the output in global logical space, so a point is the window's origin plus the
//! window-local point (measured: a click at global (814,198) landed at surface-local (100,50)
//! in a window at (714,148)).

use cua_action::{Coord, Point, WindowSpace};
use cua_desktop::{GlobalCoord, GlobalRect};

/// A point in the EI region's space. Lives in this module only: nothing else in cua names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalPoint(pub i32, pub i32);

/// How window-local points map to the EI region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionMap {
    /// The region is the window.
    Identity,
    /// The region is global; the window sits at this frame.
    Offset(GlobalRect),
}

impl RegionMap {
    /// The EI point for a window-local point. Coordinates beyond `i32` saturate.
    pub fn region_of(self, p: Point<WindowSpace>) -> GlobalPoint {
        let to_i32 = |c: Coord| i32::try_from(c.0).unwrap_or(i32::MAX);
        let (x, y) = (to_i32(p.x), to_i32(p.y));
        match self {
            RegionMap::Identity => GlobalPoint(x, y),
            RegionMap::Offset(GlobalRect { x: ox, y: oy, .. }) => {
                GlobalPoint(ox.0.saturating_add(x), oy.0.saturating_add(y))
            }
        }
    }

    /// The window-local point for an EI point, or `None` when it falls left of or above the
    /// window.
    pub fn window_of(self, p: GlobalPoint) -> Option<Point<WindowSpace>> {
        let (dx, dy) = match self {
            RegionMap::Identity => (p.0, p.1),
            RegionMap::Offset(GlobalRect {
                x: GlobalCoord(ox),
                y: GlobalCoord(oy),
                ..
            }) => (p.0.checked_sub(ox)?, p.1.checked_sub(oy)?),
        };
        Some(Point::new(
            Coord(u32::try_from(dx).ok()?),
            Coord(u32::try_from(dy).ok()?),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use porter_core::Px;

    fn frame(x: i32, y: i32) -> GlobalRect {
        GlobalRect {
            x: GlobalCoord(x),
            y: GlobalCoord(y),
            w: Px(400),
            h: Px(300),
        }
    }

    #[test]
    fn stock_region_transform_round_trips() {
        struct Case {
            name: &'static str,
            map: RegionMap,
            local: (u32, u32),
            region: (i32, i32),
        }
        let cases = [
            Case {
                name: "the fork's window region",
                map: RegionMap::Identity,
                local: (100, 50),
                region: (100, 50),
            },
            Case {
                name: "measured stock click",
                map: RegionMap::Offset(frame(714, 148)),
                local: (100, 50),
                region: (814, 198),
            },
            Case {
                name: "window left of the origin",
                map: RegionMap::Offset(frame(-200, 10)),
                local: (30, 5),
                region: (-170, 15),
            },
        ];
        for Case {
            name,
            map,
            local: (wx, wy),
            region: (gx, gy),
        } in cases
        {
            let local = Point::new(Coord(wx), Coord(wy));
            assert_eq!(map.region_of(local), GlobalPoint(gx, gy), "{name}");
            assert_eq!(
                map.window_of(GlobalPoint(gx, gy)),
                Some(local),
                "{name} back"
            );
        }
    }

    #[test]
    fn a_point_left_of_the_window_has_no_window_coordinate() {
        let map = RegionMap::Offset(frame(714, 148));
        assert_eq!(map.window_of(GlobalPoint(700, 200)), None);
    }
}
