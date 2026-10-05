#![cfg(feature = "wayland")]
//! The stock capture backend against an in-process compositor (wayland-server, a socketpair):
//! formats, frames, masks, pixel-diff damage, resizes and a window that goes away.

mod support;

use cua_action::{Coord, DeviceSize, PixelFormat, Point, Rect, Scale120, Size, WindowSpace};
use cua_desktop::{
    EiOrigin, GlobalCoord, GlobalRect, LeaseGrant, LeaseId, Pid, SeatIsolation, Stamp,
    ToplevelIdent, ToplevelInfo, WindowTitle, WindowTrust, WorkspaceRef, WorkspaceShown,
};
use maskcap::{
    CaptureError, CaptureSession, DamageSource, FrameWant, MaskReason, MaskRegion,
    StampClock, StockToplevelCapture, WindowCapture, WholeMask,
};
use porter_core::{AppName, Px};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use support::{FakeCompositor, Window};
use wayland_client::Connection;
use wayland_server::protocol::wl_shm::Format;

fn grant(ident: &str) -> LeaseGrant {
    LeaseGrant {
        id: LeaseId(1),
        window: ToplevelInfo {
            ident: ToplevelIdent::parse(ident).expect("an identifier"),
            app: AppName::parse("org.gnome.Calculator").expect("an app"),
            title: WindowTitle("Calculator".into()),
            pid: Pid(0),
            geometry: GlobalRect { x: GlobalCoord(0), y: GlobalCoord(0), w: Px(4), h: Px(3) },
            workspace: WorkspaceRef { id: String::new(), shown: WorkspaceShown::Shown },
            state: BTreeSet::new(),
            trust: WindowTrust::Native,
        },
        seat: SeatIsolation::SharedSeat,
        ei: EiOrigin::SenderSocket,
    }
}

fn capture(server: &mut FakeCompositor) -> StockToplevelCapture {
    StockToplevelCapture::over(Connection::from_socket(server.client()).expect("connect"))
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

/// A clock the test sets.
#[derive(Debug, Default)]
struct SetClock(AtomicU64);

impl StampClock for SetClock {
    fn now(&self) -> Stamp {
        Stamp(self.0.load(Ordering::Relaxed))
    }
}

const RED: [u8; 4] = [255, 0, 0, 255];

/// `pixels` with the pixel at (x, y) set to `rgba`, for a window `w` wide.
fn with_pixel(mut pixels: Vec<u8>, w: u32, x: u32, y: u32, rgba: [u8; 4]) -> Vec<u8> {
    let at = ((y * w + x) * 4) as usize;
    pixels[at..at + 4].copy_from_slice(&rgba);
    pixels
}

#[tokio::test]
async fn a_frame_carries_the_windows_pixels_at_logical_resolution() {
    let window = Window::solid("w1", (4, 3), RED);
    let mut server = FakeCompositor::start(vec![window.clone()]);
    let clock = Arc::new(SetClock(AtomicU64::new(41)));
    let capture = capture(&mut server).with_clock(clock);
    let mut session = capture.open(&grant("w1")).await.expect("open");

    let frame = session.frame(FrameWant::Now, &[]).await.expect("frame");
    assert_eq!(frame.pixels.format, PixelFormat::Abgr8888, "the 8-bit format, not the 10-bit pair");
    assert_eq!(frame.pixels.size, DeviceSize { w: 4, h: 3 });
    assert_eq!(frame.pixels.stride, 16);
    assert_eq!(&*frame.pixels.data, &window.pixels[..]);
    assert_eq!(frame.logical, Size::new(Coord(4), Coord(3)));
    assert_eq!(frame.scale, Scale120(120));
    assert_eq!(frame.at, Stamp(41));
    assert_eq!(frame.seq.0, 1);
    assert_eq!(frame.masks.applied_by, cua_desktop::MaskWhere::Cuad);
    assert_eq!(frame.masks.whole, WholeMask::None);
    assert_eq!(frame.damage.len(), 1, "the first frame is all new");
    assert_eq!(frame.damage[0].size, Size::new(Coord(4), Coord(3)));

    let again = session.frame(FrameWant::Now, &[]).await.expect("frame");
    assert_eq!(again.seq.0, 2);
    assert!(again.damage.is_empty(), "nothing changed, whatever the compositor reports");
}

#[tokio::test]
async fn hints_are_blacked_by_cuad_and_reported() {
    let mut server = FakeCompositor::start(vec![Window::solid("w1", (4, 3), RED)]);
    let capture = capture(&mut server);
    let mut session = capture.open(&grant("w1")).await.expect("open");
    let hint = region(1, 1, 2, 1);
    let frame = session.frame(FrameWant::Now, &[hint]).await.expect("frame");
    let at = |x: usize, y: usize| &frame.pixels.data[(y * 16 + x * 4)..(y * 16 + x * 4 + 4)];
    assert_eq!(at(1, 1), &[0, 0, 0, 255]);
    assert_eq!(at(2, 1), &[0, 0, 0, 255]);
    assert_eq!(at(0, 1), &RED);
    assert_eq!(at(3, 1), &RED);
    assert_eq!(frame.masks.regions, vec![hint]);
}

#[tokio::test]
async fn after_damage_waits_for_a_pixel_diff_and_names_the_changed_box() {
    let window = Window::solid("w1", (4, 3), RED);
    let mut server = FakeCompositor::start(vec![window.clone()]);
    let capture = capture(&mut server);
    let mut session = capture.open(&grant("w1")).await.expect("open");
    session.frame(FrameWant::Now, &[]).await.expect("the first frame");

    let waiting = tokio::spawn(async move {
        let frame = session.frame(FrameWant::AfterDamage, &[]).await;
        (session, frame)
    });
    // The compositor reports full damage for every frame; only the pixels say what changed.
    server.show(0, with_pixel(window.pixels.clone(), 4, 2, 1, [0, 255, 0, 255]));
    let (_session, frame) = waiting.await.expect("task");
    let frame = frame.expect("frame");
    assert_eq!(frame.damage.len(), 1);
    assert_eq!(frame.damage[0].origin, Point::new(Coord(2), Coord(1)));
    assert_eq!(frame.damage[0].size, Size::new(Coord(1), Coord(1)));
    assert!(server.frames_served() >= 2);
}

#[tokio::test]
async fn damage_is_the_next_pixel_diff() {
    let window = Window::solid("w1", (4, 3), RED);
    let mut server = FakeCompositor::start(vec![window.clone()]);
    let clock = Arc::new(SetClock(AtomicU64::new(7)));
    let capture = capture(&mut server).with_clock(clock);
    let mut session = capture.open(&grant("w1")).await.expect("open");

    // The first look at a window is all new.
    let first = session.damage().await.expect("damage");
    assert_eq!(first.source, DamageSource::PixelDiff);
    assert_eq!(first.region.size, Size::new(Coord(4), Coord(3)));
    assert_eq!(first.at, Stamp(7));

    let waiting = tokio::spawn(async move {
        let damage = session.damage().await;
        (session, damage)
    });
    server.show(0, with_pixel(window.pixels.clone(), 4, 0, 2, [0, 0, 255, 255]));
    let (_session, damage) = waiting.await.expect("task");
    let damage = damage.expect("damage");
    assert_eq!(damage.region.origin, Point::new(Coord(0), Coord(2)));
    assert_eq!(damage.region.size, Size::new(Coord(1), Coord(1)));
}

#[tokio::test]
async fn a_resized_window_gets_a_new_buffer() {
    let mut server = FakeCompositor::start(vec![Window::solid("w1", (4, 3), RED)]);
    let capture = capture(&mut server);
    let mut session = capture.open(&grant("w1")).await.expect("open");
    session.frame(FrameWant::Now, &[]).await.expect("frame");

    let bigger = Window::solid("w1", (6, 5), [1, 2, 3, 255]);
    server.resize(0, bigger.size, bigger.pixels.clone());
    let frame = session.frame(FrameWant::Now, &[]).await.expect("frame");
    assert_eq!(frame.pixels.size, DeviceSize { w: 6, h: 5 });
    assert_eq!(&*frame.pixels.data, &bigger.pixels[..]);
    assert_eq!(frame.logical, Size::new(Coord(6), Coord(5)));
    assert_eq!(frame.damage[0].size, Size::new(Coord(6), Coord(5)), "a new size differs everywhere");
}

#[tokio::test]
async fn a_window_that_closes_ends_its_session() {
    let mut server = FakeCompositor::start(vec![Window::solid("w1", (4, 3), RED)]);
    let capture = capture(&mut server);
    let mut session = capture.open(&grant("w1")).await.expect("open");
    session.frame(FrameWant::Now, &[]).await.expect("frame");
    server.close(0);
    assert_eq!(
        session.frame(FrameWant::Now, &[]).await.unwrap_err(),
        CaptureError::SourceGone
    );
}

#[tokio::test]
async fn opening_what_is_not_there_or_cannot_be_read_is_refused() {
    let mut server = FakeCompositor::start(vec![Window::solid("w1", (4, 3), RED)]);
    let capture = capture(&mut server);
    assert_eq!(
        capture.open(&grant("w9")).await.unwrap_err(),
        CaptureError::SourceGone
    );

    server.offer(vec![Format::Abgr2101010, Format::Xbgr2101010]);
    assert_eq!(
        capture.open(&grant("w1")).await.unwrap_err(),
        CaptureError::Format,
        "only 10-bit formats are offered"
    );

    server.offer(vec![Format::Xbgr8888]);
    let mut session = capture.open(&grant("w1")).await.expect("open");
    let frame = session.frame(FrameWant::Now, &[]).await.expect("frame");
    assert_eq!(frame.pixels.format, PixelFormat::Xbgr8888);
}

#[tokio::test]
async fn two_sessions_share_one_connection() {
    let mut server = FakeCompositor::start(vec![
        Window::solid("w1", (4, 3), RED),
        Window::solid("w2", (2, 2), [9, 9, 9, 255]),
    ]);
    let capture = capture(&mut server);
    let mut a = capture.open(&grant("w1")).await.expect("open");
    let mut b = capture.open(&grant("w2")).await.expect("open");
    let (fa, fb) = tokio::join!(a.frame(FrameWant::Now, &[]), b.frame(FrameWant::Now, &[]));
    assert_eq!(fa.unwrap().pixels.size, DeviceSize { w: 4, h: 3 });
    assert_eq!(fb.unwrap().pixels.size, DeviceSize { w: 2, h: 2 });
}
