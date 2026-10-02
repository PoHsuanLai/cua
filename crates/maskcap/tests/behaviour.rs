//! What the masking and settling functions must do. They are frozen with `todo!()` bodies, so
//! each test is ignored with the work that closes it and carries the real assertions.

use cua_action::{Coord, DeviceSize, PixelFormat, Point, Rect, Scale120, Size, WindowSpace};
use cua_desktop::{Ms, Stamp};
use maskcap::{
    DamageAt, DamageSource, MaskReason, MaskRegion, PixelBuf, Settle, apply_masks, digest,
    pixel_diff, settle,
};
use std::sync::Arc;

/// A `w` by `h` buffer of opaque white pixels (4 bytes each, no row padding).
fn white(w: u32, h: u32) -> PixelBuf {
    PixelBuf {
        format: PixelFormat::Xbgr8888,
        size: DeviceSize { w, h },
        stride: w * 4,
        data: Arc::from(vec![255u8; (w * h * 4) as usize]),
    }
}

fn pixel(buf: &PixelBuf, x: u32, y: u32) -> [u8; 4] {
    let at = (y * buf.stride + x * 4) as usize;
    [
        buf.data[at],
        buf.data[at + 1],
        buf.data[at + 2],
        buf.data[at + 3],
    ]
}

fn region(x: u32, y: u32, w: u32, h: u32) -> MaskRegion {
    MaskRegion {
        rect: Rect {
            origin: Point::<WindowSpace>::new(Coord(x), Coord(y)),
            size: Size::new(Coord(w), Coord(h)),
        },
        why: MaskReason::PasswordField,
    }
}

fn damage_at(ms: u64) -> DamageAt {
    DamageAt {
        at: Stamp(ms),
        region: Rect {
            origin: Point::new(Coord(0), Coord(0)),
            size: Size::new(Coord(1), Coord(1)),
        },
        source: DamageSource::PixelDiff,
    }
}

#[test]
#[ignore = "closes with the maskcap fill (apply_masks)"]
fn apply_masks_blacks_exact_device_pixels_at_scale_150() {
    // At 1.5 (180 in 120ths) a 2-lpx region at logical (2,2) covers device pixels 3..6.
    let (masked, applied) = apply_masks(white(30, 30), Scale120(180), &[region(2, 2, 2, 2)]);
    for y in 0..30 {
        for x in 0..30 {
            let inside = (3..6).contains(&x) && (3..6).contains(&y);
            let want = if inside {
                [0, 0, 0, 255]
            } else {
                [255, 255, 255, 255]
            };
            assert_eq!(pixel(&masked, x, y), want, "({x},{y})");
        }
    }
    assert_eq!(applied, vec![region(2, 2, 2, 2)]);
}

#[test]
#[ignore = "closes with the maskcap fill (apply_masks)"]
fn a_region_past_the_edge_is_clamped_and_reported_as_applied() {
    let (masked, applied) = apply_masks(white(10, 10), Scale120(120), &[region(8, 8, 10, 10)]);
    assert_eq!(pixel(&masked, 9, 9), [0, 0, 0, 255]);
    assert_eq!(pixel(&masked, 7, 7), [255, 255, 255, 255]);
    assert_eq!(applied, vec![region(8, 8, 2, 2)]);
}

#[test]
#[ignore = "closes with the maskcap fill (digest)"]
fn digest_differs_on_one_pixel_and_ignores_stride_padding() {
    let a = white(8, 8);
    let mut bytes = a.data.to_vec();
    bytes[0] = 0;
    let b = PixelBuf {
        data: Arc::from(bytes),
        ..a.clone()
    };
    assert_ne!(digest(&a), digest(&b));
    assert_eq!(digest(&a), digest(&a.clone()));
    let mut padded = Vec::new();
    for row in a.data.chunks(32) {
        padded.extend_from_slice(row);
        padded.extend_from_slice(&[7, 7, 7, 7]);
    }
    let padded = PixelBuf {
        stride: 36,
        data: Arc::from(padded),
        ..a.clone()
    };
    assert_eq!(digest(&a), digest(&padded));
}

#[test]
#[ignore = "closes with the maskcap fill (settle)"]
fn settle_quiet_waiting_timeout_table() {
    struct Row {
        name: &'static str,
        events: Vec<DamageAt>,
        now: u64,
        want: Settle,
    }
    let rows = [
        Row {
            name: "nothing changed since the action",
            events: vec![],
            now: 300,
            want: Settle::Quiet,
        },
        Row {
            name: "still changing",
            events: vec![damage_at(200)],
            now: 300,
            want: Settle::Waiting { wake: Stamp(450) },
        },
        Row {
            name: "quiet long enough",
            events: vec![damage_at(10)],
            now: 300,
            want: Settle::Quiet,
        },
        Row {
            name: "timed out while changing",
            events: vec![damage_at(2900)],
            now: 3000,
            want: Settle::TimedOut,
        },
    ];
    for row in rows {
        let got = settle(&row.events, Ms(250), Ms(3000), Stamp(0), Stamp(row.now));
        assert_eq!(got, row.want, "{}", row.name);
    }
}

#[test]
#[ignore = "closes with the maskcap fill (pixel_diff)"]
fn pixel_diff_bounds_the_changed_pixels() {
    let a = white(10, 10);
    assert_eq!(pixel_diff(&a, &a), None);
    let mut bytes = a.data.to_vec();
    for (x, y) in [(2u32, 3u32), (4, 6)] {
        bytes[((y * 40) + x * 4) as usize] = 0;
    }
    let b = PixelBuf {
        data: Arc::from(bytes),
        ..a.clone()
    };
    assert_eq!(
        pixel_diff(&a, &b),
        Some(Rect {
            origin: Point::new(Coord(2), Coord(3)),
            size: Size::new(Coord(3), Coord(4)),
        })
    );
}
