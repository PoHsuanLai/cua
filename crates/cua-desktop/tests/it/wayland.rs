#![cfg(feature = "wayland")]
//! The stock COSMIC backend against an in-process compositor (wayland-server, a socketpair):
//! what `windows` reports, what `next_event` hears, and the stock in-place lease.

use crate::support::{FakeCompositor, Offer, Spec};
use cua_desktop::{
    BackendKind, CosmicDesktop, Desktop, DesktopError, DesktopEvent, DeviceTypes, EiOrigin,
    LeaseControl, LeaseRequest, ReleasePlacement, RevokeReason, RunMode, SeatIsolation,
    SenderSocket, SocketFuture, ToplevelIdent, ToplevelState, WindowTrust, WorkspaceShown,
};
use porter_core::Px;
use prov::SpaceId;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use wayland_client::Connection;

fn spec(identifier: &'static str, app_id: &'static str, title: &'static str) -> Spec {
    Spec {
        identifier,
        app_id,
        title,
        states: vec![2],
        geometry: (214, 100, 852, 532),
        workspace: None,
    }
}

fn desktop(server: &mut FakeCompositor) -> CosmicDesktop {
    let connection = Connection::from_socket(server.client()).expect("connect");
    CosmicDesktop::over(connection, BackendKind::CosmicStock)
}

fn ident(text: &str) -> ToplevelIdent {
    ToplevelIdent::parse(text).expect("an identifier")
}

/// Hands out one end of a socketpair and remembers what it was asked for.
#[derive(Debug, Default)]
struct Sockets {
    asked: Mutex<Vec<DeviceTypes>>,
    refuse: Mutex<bool>,
    kept: Mutex<Vec<UnixStream>>,
}

impl SenderSocket for Sockets {
    fn sender_socket(&self, devices: DeviceTypes) -> SocketFuture<'_> {
        Box::pin(async move {
            self.asked.lock().unwrap().push(devices);
            if *self.refuse.lock().unwrap() {
                return Err(DesktopError::Protocol("access denied".into()));
            }
            let (ours, theirs) = UnixStream::pair().expect("pair");
            self.kept.lock().unwrap().push(theirs);
            Ok(OwnedFd::from(ours))
        })
    }
}

fn request(window: &str, mode: RunMode) -> LeaseRequest {
    LeaseRequest {
        window: ident(window),
        mode,
        space: SpaceId::parse("desktop").expect("a space"),
        placement: ReleasePlacement::ReturnToUser,
    }
}

#[tokio::test]
async fn windows_reports_what_the_compositor_says() {
    let mut server = FakeCompositor::start(Offer::Cosmic);
    let shown = server.workspace("ws-1", true);
    let hidden = server.workspace("ws-2", false);
    server.add(Spec {
        workspace: Some(shown),
        states: vec![0, 2],
        ..spec("id-term", "com.system76.CosmicTerm", "Terminal")
    });
    server.add(Spec {
        workspace: Some(hidden),
        states: vec![1],
        geometry: (0, 0, 1280, 800),
        ..spec("id-konsole", "konsole", "~ : konsole")
    });
    server.add(spec("id-bar", "org.quire.Shell", "Bar"));
    let desktop = desktop(&mut server);

    let windows = desktop.windows().await.expect("windows");
    assert_eq!(windows.len(), 3);

    let term = &windows[0];
    assert_eq!(term.ident, ident("id-term"));
    assert_eq!(term.app.as_str(), "com.system76.CosmicTerm");
    assert_eq!(term.title.0, "Terminal");
    assert_eq!(term.geometry.x.0, 214);
    assert_eq!(term.geometry.y.0, 100);
    assert_eq!((term.geometry.w, term.geometry.h), (Px(852), Px(532)));
    assert_eq!(term.workspace.id, "ws-1");
    assert_eq!(term.workspace.shown, WorkspaceShown::Shown);
    assert_eq!(
        term.state.iter().copied().collect::<Vec<_>>(),
        vec![ToplevelState::Maximized, ToplevelState::Activated]
    );
    assert_eq!(term.trust, WindowTrust::Native);
    assert_eq!(term.pid.0, 0, "no protocol here says which process owns it");

    let konsole = &windows[1];
    assert_eq!(konsole.app.as_str(), "app.konsole");
    assert_eq!(konsole.workspace.shown, WorkspaceShown::Hidden);
    assert_eq!(
        konsole.state.iter().copied().collect::<Vec<_>>(),
        vec![ToplevelState::Minimized]
    );

    let bar = &windows[2];
    assert_eq!(bar.trust, WindowTrust::Shell);
    assert_eq!(bar.workspace.id, "", "no workspace was announced");
    assert_eq!(bar.workspace.shown, WorkspaceShown::Shown);
}

#[tokio::test]
async fn windows_follow_the_workspace_and_the_toplevels_that_come_and_go() {
    let mut server = FakeCompositor::start(Offer::Cosmic);
    let ws = server.workspace("ws-1", true);
    let first = server.add(Spec {
        workspace: Some(ws),
        ..spec("id-1", "org.gnome.Calculator", "Calculator")
    });
    let desktop = desktop(&mut server);
    assert_eq!(desktop.windows().await.unwrap().len(), 1);

    server.add(spec("id-2", "org.gnome.Nautilus", "Files"));
    assert_eq!(desktop.windows().await.unwrap().len(), 2);

    server.show(ws, false);
    let windows = desktop.windows().await.unwrap();
    assert_eq!(windows[0].workspace.shown, WorkspaceShown::Hidden);

    server.close(first);
    let windows = desktop.windows().await.unwrap();
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].ident, ident("id-2"));
}

#[tokio::test]
async fn a_compositor_without_the_toplevel_list_is_a_protocol_error() {
    let mut server = FakeCompositor::start(Offer::Nothing);
    let desktop = desktop(&mut server);
    let error = desktop.windows().await.expect_err("no list");
    assert!(matches!(error, DesktopError::Protocol(_)), "{error:?}");
}

#[tokio::test]
async fn a_list_without_cosmic_info_still_names_the_windows() {
    let mut server = FakeCompositor::start(Offer::ListOnly);
    server.add(spec("id-1", "org.gnome.Calculator", "Calculator"));
    let desktop = desktop(&mut server);
    let windows = desktop.windows().await.unwrap();
    assert_eq!(windows.len(), 1);
    assert_eq!(
        windows[0].geometry.w,
        Px(0),
        "no geometry without cosmic info"
    );
    assert!(windows[0].state.is_empty());
}

#[tokio::test]
async fn next_event_hears_geometry_changes_and_closes_but_not_the_first_geometry() {
    let mut server = FakeCompositor::start(Offer::Cosmic);
    let top = server.add(spec("id-1", "org.gnome.Calculator", "Calculator"));
    let desktop = desktop(&mut server);
    desktop.windows().await.unwrap();

    server.move_to(top, (10, 20, 300, 200));
    let event = desktop.next_event().await;
    let DesktopEvent::GeometryChanged(who, rect) = event else {
        panic!("{event:?}");
    };
    assert_eq!(who, ident("id-1"));
    assert_eq!(
        (rect.x.0, rect.y.0, rect.w, rect.h),
        (10, 20, Px(300), Px(200))
    );

    server.close(top);
    assert_eq!(
        desktop.next_event().await,
        DesktopEvent::WindowClosed(ident("id-1"))
    );
}

#[tokio::test]
async fn next_event_and_windows_can_wait_at_once() {
    let mut server = FakeCompositor::start(Offer::Cosmic);
    let top = server.add(spec("id-1", "org.gnome.Calculator", "Calculator"));
    let desktop = Arc::new(desktop(&mut server));
    desktop.windows().await.unwrap();

    let waiting = tokio::spawn({
        let desktop = desktop.clone();
        async move { desktop.next_event().await }
    });
    // Both are in the pump now: the answer to `windows` must not swallow the event.
    server.add(spec("id-2", "org.gnome.Nautilus", "Files"));
    assert_eq!(desktop.windows().await.unwrap().len(), 2);
    server.close(top);
    assert_eq!(
        waiting.await.unwrap(),
        DesktopEvent::WindowClosed(ident("id-1"))
    );
}

#[tokio::test]
async fn a_stock_in_place_lease_hands_over_the_shared_seat_socket() {
    let mut server = FakeCompositor::start(Offer::Cosmic);
    server.add(spec("id-1", "org.gnome.Calculator", "Calculator"));
    server.add(spec("id-bar", "org.quire.Shell", "Bar"));
    let sockets = Arc::new(Sockets::default());
    let desktop = desktop(&mut server).with_sender_socket(sockets.clone());

    let leased = desktop
        .lease(request("id-1", RunMode::InPlace))
        .await
        .expect("leased");
    assert_eq!(leased.grant.seat, SeatIsolation::SharedSeat);
    assert_eq!(leased.grant.ei, EiOrigin::SenderSocket);
    assert_eq!(leased.grant.window.ident, ident("id-1"));
    assert_eq!(*sockets.asked.lock().unwrap(), vec![DeviceTypes::ALL]);
    assert_eq!(DeviceTypes::ALL, DeviceTypes(7));

    // One run per window; another window is free.
    assert_eq!(
        desktop
            .lease(request("id-1", RunMode::InPlace))
            .await
            .unwrap_err(),
        DesktopError::Busy
    );
    // Suspend, resume and glow have nothing to say to stock; they know the lease.
    let id = leased.grant.id;
    desktop
        .suspend(id, cua_desktop::Suspend::Consent)
        .await
        .unwrap();
    desktop.resume(id).await.unwrap();
    desktop
        .glow(id, cua_desktop::GlowState::Working)
        .await
        .unwrap();

    desktop.release(id).await.unwrap();
    desktop
        .release(id)
        .await
        .expect("releasing twice is not a failure");
    assert_eq!(
        desktop.resume(id).await.unwrap_err(),
        DesktopError::NoSuchWindow
    );
    let again = desktop
        .lease(request("id-1", RunMode::InPlace))
        .await
        .unwrap();
    assert_ne!(again.grant.id, id, "a lease id is not reused");
}

#[tokio::test]
async fn a_stock_lease_refuses_what_stock_cannot_do() {
    let mut server = FakeCompositor::start(Offer::Cosmic);
    server.add(spec("id-1", "org.gnome.Calculator", "Calculator"));
    server.add(spec("id-bar", "org.quire.Shell", "Bar"));
    let sockets = Arc::new(Sockets::default());
    let desktop = desktop(&mut server).with_sender_socket(sockets.clone());

    let table = [
        (
            "agent workspace",
            request("id-1", RunMode::AgentWorkspace),
            DesktopError::ModeUnsupported(RunMode::AgentWorkspace),
        ),
        (
            "nested session",
            request("id-1", RunMode::NestedSession),
            DesktopError::ModeUnsupported(RunMode::NestedSession),
        ),
        (
            "the shell",
            request("id-bar", RunMode::InPlace),
            DesktopError::NotLeasable(WindowTrust::Shell),
        ),
        (
            "no such window",
            request("id-9", RunMode::InPlace),
            DesktopError::NoSuchWindow,
        ),
    ];
    for (name, req, want) in table {
        assert_eq!(desktop.lease(req).await.unwrap_err(), want, "{name}");
    }
    assert!(
        sockets.asked.lock().unwrap().is_empty(),
        "no socket is opened for a refused lease"
    );

    // A compositor that refuses the socket grants nothing and leaves the window free.
    *sockets.refuse.lock().unwrap() = true;
    assert!(
        desktop
            .lease(request("id-1", RunMode::InPlace))
            .await
            .is_err()
    );
    *sockets.refuse.lock().unwrap() = false;
    desktop
        .lease(request("id-1", RunMode::InPlace))
        .await
        .expect("the window was not left leased");
}

#[tokio::test]
async fn a_lease_without_a_socket_source_is_a_protocol_error() {
    let mut server = FakeCompositor::start(Offer::Cosmic);
    server.add(spec("id-1", "org.gnome.Calculator", "Calculator"));
    let desktop = desktop(&mut server);
    let error = desktop
        .lease(request("id-1", RunMode::InPlace))
        .await
        .unwrap_err();
    assert!(matches!(error, DesktopError::Protocol(_)), "{error:?}");
}

#[tokio::test]
async fn losing_the_compositor_revokes_every_lease_once() {
    let mut server = FakeCompositor::start(Offer::Cosmic);
    server.add(spec("id-1", "org.gnome.Calculator", "Calculator"));
    server.add(spec("id-2", "org.gnome.Nautilus", "Files"));
    let desktop = desktop(&mut server).with_sender_socket(Arc::new(Sockets::default()));
    let a = desktop
        .lease(request("id-1", RunMode::InPlace))
        .await
        .unwrap();
    let b = desktop
        .lease(request("id-2", RunMode::InPlace))
        .await
        .unwrap();

    server.shutdown();
    let mut revoked = Vec::new();
    for _ in 0..2 {
        match desktop.next_event().await {
            DesktopEvent::LeaseRevoked { lease, why } => {
                assert_eq!(why, RevokeReason::Compositor);
                revoked.push(lease);
            }
            other => panic!("{other:?}"),
        }
    }
    revoked.sort();
    assert_eq!(revoked, vec![a.grant.id, b.grant.id]);

    // Nothing is left to say: the next call stays quiet while an ordinary call fails.
    tokio::select! {
        biased;
        event = desktop.next_event() => panic!("{event:?}"),
        windows = desktop.windows() => assert!(windows.is_err()),
    }
}
