//! What the compositor has said about toplevels and capture sessions. The handlers only record;
//! [`super`] turns the record into frames.

use crate::CaptureError;
use cua_action::{Coord, PixelFormat, Point, Rect, Size, WindowSpace};
use std::collections::BTreeMap;
use cua_desktop::{ObjectKey as Key, Rounds, SyncTag};
use wayland_client::protocol::wl_buffer::{self, WlBuffer};
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_shm::{self, Format, WlShm};
use wayland_client::protocol::wl_shm_pool::{self, WlShmPool};
use wayland_client::{Connection, Dispatch, QueueHandle, WEnum, event_created_child};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::{
    self, ExtForeignToplevelHandleV1,
};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_list_v1::{
    self, ExtForeignToplevelListV1,
};
use wayland_protocols::ext::image_capture_source::v1::client::ext_foreign_toplevel_image_capture_source_manager_v1::{
    self, ExtForeignToplevelImageCaptureSourceManagerV1,
};
use wayland_protocols::ext::image_capture_source::v1::client::ext_image_capture_source_v1::{
    self, ExtImageCaptureSourceV1,
};
use wayland_protocols::ext::image_copy_capture::v1::client::ext_image_copy_capture_frame_v1::{
    self, ExtImageCopyCaptureFrameV1, FailureReason,
};
use wayland_protocols::ext::image_copy_capture::v1::client::ext_image_copy_capture_manager_v1::{
    self, ExtImageCopyCaptureManagerV1, Options,
};
use wayland_protocols::ext::image_copy_capture::v1::client::ext_image_copy_capture_session_v1::{
    self, ExtImageCopyCaptureSessionV1,
};

#[derive(Debug)]
struct Top {
    handle: ExtForeignToplevelHandleV1,
    ident: Option<String>,
}

/// What the session has said of its buffer constraints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Constraints {
    /// No `done` yet.
    Pending,
    /// The constraints in `size` and `formats` are complete.
    Known,
    /// The compositor ended the session (the window is gone).
    Stopped,
}

/// How the frame in flight stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FrameSlot {
    /// None asked for.
    Idle,
    /// Asked; no answer yet.
    Waiting,
    /// Ready, with the damage the compositor reported.
    Ready(Vec<Rect<WindowSpace>>),
    /// Failed.
    Failed(Failure),
}

/// Why a frame failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The buffer no longer fits: the session says new constraints, then `done`.
    Constraints,
    /// The session ended.
    Stopped,
    /// The compositor did not say.
    Unknown,
}

/// One capture session's record.
#[derive(Debug)]
pub(crate) struct SessionRec {
    pub proxy: ExtImageCopyCaptureSessionV1,
    pub constraints: Constraints,
    /// Counts the `done` events: a buffer made at another epoch may no longer fit.
    pub epoch: u32,
    pub size: (u32, u32),
    pub formats: Vec<WEnum<Format>>,
    next_size: (u32, u32),
    next_formats: Vec<WEnum<Format>>,
    pub frame: FrameSlot,
    damage: Vec<Rect<WindowSpace>>,
}

/// Everything the handlers record.
#[derive(Debug)]
pub(crate) struct CapState {
    qh: QueueHandle<CapState>,
    connection: Connection,
    list: Option<ExtForeignToplevelListV1>,
    sources: Option<ExtForeignToplevelImageCaptureSourceManagerV1>,
    copy: Option<ExtImageCopyCaptureManagerV1>,
    shm: Option<WlShm>,
    toplevels: BTreeMap<Key, Top>,
    pub sessions: BTreeMap<Key, SessionRec>,
    rounds: Rounds,
}

impl CapState {
    pub(crate) fn new(connection: &Connection, qh: QueueHandle<CapState>) -> Self {
        connection.display().get_registry(&qh, ());
        Self {
            qh,
            connection: connection.clone(),
            list: None,
            sources: None,
            copy: None,
            shm: None,
            toplevels: BTreeMap::new(),
            sessions: BTreeMap::new(),
            rounds: Rounds::default(),
        }
    }

    pub(crate) fn qh(&self) -> &QueueHandle<CapState> {
        &self.qh
    }

    pub(crate) fn shm(&self) -> Result<&WlShm, CaptureError> {
        self.shm
            .as_ref()
            .ok_or_else(|| CaptureError::Protocol("the compositor offers no wl_shm".into()))
    }

    pub(crate) fn sync(&mut self) -> u64 {
        self.rounds.ask(&self.connection, &self.qh)
    }

    pub(crate) fn synced(&self, n: u64) -> bool {
        self.rounds.reached(n)
    }

    /// Opens a capture session over the toplevel named `ident`.
    pub(crate) fn open_session(&mut self, ident: &str) -> Result<Key, CaptureError> {
        let (Some(sources), Some(copy)) = (&self.sources, &self.copy) else {
            return Err(CaptureError::Protocol(
                "the compositor offers no toplevel capture (ext-image-copy-capture)".into(),
            ));
        };
        let top = self
            .toplevels
            .values()
            .find(|top| top.ident.as_deref() == Some(ident))
            .ok_or(CaptureError::SourceGone)?;
        let source = sources.create_source(&top.handle, &self.qh, ());
        let proxy = copy.create_session(&source, Options::empty(), &self.qh, ());
        let key = Key::of(&proxy);
        source.destroy();
        self.sessions.insert(
            key,
            SessionRec {
                proxy,
                constraints: Constraints::Pending,
                epoch: 0,
                size: (0, 0),
                formats: Vec::new(),
                next_size: (0, 0),
                next_formats: Vec::new(),
                frame: FrameSlot::Idle,
                damage: Vec::new(),
            },
        );
        Ok(key)
    }

    /// Asks for one frame into `buffer`.
    pub(crate) fn request_frame(&mut self, key: Key, buffer: &WlBuffer, size: (u32, u32)) {
        let Some(rec) = self.sessions.get_mut(&key) else {
            return;
        };
        rec.frame = FrameSlot::Waiting;
        rec.damage.clear();
        let frame = rec.proxy.create_frame(&self.qh, key);
        frame.attach_buffer(buffer);
        let (w, h) = (
            i32::try_from(size.0).unwrap_or(i32::MAX),
            i32::try_from(size.1).unwrap_or(i32::MAX),
        );
        frame.damage_buffer(0, 0, w, h);
        frame.capture();
    }

    /// Ends a session's objects.
    pub(crate) fn close_session(&mut self, key: Key) {
        if let Some(rec) = self.sessions.remove(&key) {
            rec.proxy.destroy();
        }
    }
}

/// The format to ask for among those offered: 8-bit, the order stock cosmic-comp prefers.
pub(crate) fn pick_format(offered: &[WEnum<Format>]) -> Option<(Format, PixelFormat)> {
    [
        (Format::Abgr8888, PixelFormat::Abgr8888),
        (Format::Xbgr8888, PixelFormat::Xbgr8888),
        (Format::Argb8888, PixelFormat::Argb8888),
        (Format::Xrgb8888, PixelFormat::Xrgb8888),
    ]
    .into_iter()
    .find(|(wire, _)| offered.contains(&WEnum::Value(*wire)))
}

impl Dispatch<WlRegistry, ()> for CapState {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_registry::Event::Global {
            name, interface, ..
        } = event
        else {
            return;
        };
        match interface.as_str() {
            "ext_foreign_toplevel_list_v1" => state.list = Some(registry.bind(name, 1, qh, ())),
            "ext_foreign_toplevel_image_capture_source_manager_v1" => {
                state.sources = Some(registry.bind(name, 1, qh, ()));
            }
            "ext_image_copy_capture_manager_v1" => {
                state.copy = Some(registry.bind(name, 1, qh, ()))
            }
            "wl_shm" => state.shm = Some(registry.bind(name, 1, qh, ())),
            _ => {}
        }
    }
}

impl Dispatch<WlCallback, SyncTag> for CapState {
    fn event(
        state: &mut Self,
        _: &WlCallback,
        event: wl_callback::Event,
        _: &SyncTag,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let wl_callback::Event::Done { .. } = event else {
            return;
        };
        state.rounds.answered();
    }
}

impl Dispatch<ExtForeignToplevelListV1, ()> for CapState {
    fn event(
        state: &mut Self,
        _: &ExtForeignToplevelListV1,
        event: ext_foreign_toplevel_list_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } = event else {
            return;
        };
        state.toplevels.insert(
            Key::of(&toplevel),
            Top {
                handle: toplevel,
                ident: None,
            },
        );
    }

    event_created_child!(CapState, ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (ExtForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ExtForeignToplevelHandleV1, ()> for CapState {
    fn event(
        state: &mut Self,
        handle: &ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_foreign_toplevel_handle_v1::Event;
        let key = &Key::of(handle);
        match event {
            Event::Identifier { identifier } => {
                if let Some(top) = state.toplevels.get_mut(key) {
                    top.ident = Some(identifier);
                }
            }
            Event::Closed => {
                state.toplevels.remove(key);
                handle.destroy();
            }
            _ => {}
        }
    }
}

macro_rules! ignore_events {
    ($($proxy:ty, $event:ty, $data:ty;)*) => {$(
        impl Dispatch<$proxy, $data> for CapState {
            fn event(_: &mut Self, _: &$proxy, _: $event, _: &$data, _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}

ignore_events! {
    WlShm, wl_shm::Event, ();
    WlShmPool, wl_shm_pool::Event, ();
    WlBuffer, wl_buffer::Event, ();
    ExtImageCaptureSourceV1, ext_image_capture_source_v1::Event, ();
    ExtForeignToplevelImageCaptureSourceManagerV1, ext_foreign_toplevel_image_capture_source_manager_v1::Event, ();
    ExtImageCopyCaptureManagerV1, ext_image_copy_capture_manager_v1::Event, ();
}

impl Dispatch<ExtImageCopyCaptureSessionV1, ()> for CapState {
    fn event(
        state: &mut Self,
        session: &ExtImageCopyCaptureSessionV1,
        event: ext_image_copy_capture_session_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_image_copy_capture_session_v1::Event;
        let Some(rec) = state.sessions.get_mut(&Key::of(session)) else {
            return;
        };
        match event {
            Event::BufferSize { width, height } => rec.next_size = (width, height),
            Event::ShmFormat { format } => rec.next_formats.push(format),
            Event::Done => {
                rec.size = rec.next_size;
                rec.formats = std::mem::take(&mut rec.next_formats);
                rec.epoch += 1;
                if rec.constraints == Constraints::Pending {
                    rec.constraints = Constraints::Known;
                }
            }
            Event::Stopped => rec.constraints = Constraints::Stopped,
            _ => {}
        }
    }
}

impl Dispatch<ExtImageCopyCaptureFrameV1, Key> for CapState {
    fn event(
        state: &mut Self,
        frame: &ExtImageCopyCaptureFrameV1,
        event: ext_image_copy_capture_frame_v1::Event,
        key: &Key,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_image_copy_capture_frame_v1::Event;
        let Some(rec) = state.sessions.get_mut(key) else {
            return;
        };
        match event {
            Event::Damage {
                x,
                y,
                width,
                height,
            } => {
                let to = |n: i32| Coord(u32::try_from(n).unwrap_or(0));
                rec.damage.push(Rect {
                    origin: Point::new(to(x), to(y)),
                    size: Size::new(to(width), to(height)),
                });
            }
            Event::Ready => {
                rec.frame = FrameSlot::Ready(std::mem::take(&mut rec.damage));
                frame.destroy();
            }
            Event::Failed { reason } => {
                rec.frame = FrameSlot::Failed(match reason {
                    WEnum::Value(FailureReason::BufferConstraints) => Failure::Constraints,
                    WEnum::Value(FailureReason::Stopped) => Failure::Stopped,
                    _ => Failure::Unknown,
                });
                frame.destroy();
            }
            _ => {}
        }
    }
}
