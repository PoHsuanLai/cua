//! What the compositor has said so far: the toplevels (`ext_foreign_toplevel_list_v1` and the
//! COSMIC info beside it), the workspaces (`ext_workspace_v1`) and the events cuad has not
//! been told yet. The handlers only record; [`crate::wayland`] turns the record into answers.

use super::proto::client::zcosmic_toplevel_handle_v1::{self, ZcosmicToplevelHandleV1};
use super::proto::client::zcosmic_toplevel_info_v1::{self, ZcosmicToplevelInfoV1};
use crate::{DesktopEvent, GlobalCoord, GlobalRect, ToplevelIdent, ToplevelState};
use porter_core::Px;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum, event_created_child};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::{
    self, ExtForeignToplevelHandleV1,
};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_list_v1::{
    self, ExtForeignToplevelListV1,
};
use wayland_protocols::ext::workspace::v1::client::ext_workspace_group_handle_v1::{
    self, ExtWorkspaceGroupHandleV1,
};
use wayland_protocols::ext::workspace::v1::client::ext_workspace_handle_v1::{
    self, ExtWorkspaceHandleV1, State as WorkspaceBits,
};
use wayland_protocols::ext::workspace::v1::client::ext_workspace_manager_v1::{
    self, ExtWorkspaceManagerV1,
};

/// The version of `zcosmic_toplevel_info_v1` this code binds, and the oldest it accepts: from
/// 3 on a toplevel's workspaces are `ext_workspace_v1` handles.
const INFO_VERSION: u32 = 3;

static NEXT_KEY: AtomicU64 = AtomicU64::new(1);

/// Names one toplevel or workspace across its handles (the object ids belong to the wire).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct Key(u64);

impl Key {
    pub(crate) fn fresh() -> Self {
        Key(NEXT_KEY.fetch_add(1, Ordering::Relaxed))
    }
}

/// Tags a `wl_display.sync` callback.
#[derive(Debug)]
pub(crate) struct SyncTag;

/// A workspace as `ext_workspace_v1` describes it.
#[derive(Debug, Default)]
pub(crate) struct Workspace {
    pub id: String,
    pub active: bool,
}

/// One toplevel, as far as the compositor has told it.
#[derive(Debug)]
pub(crate) struct Toplevel {
    pub ext: ExtForeignToplevelHandleV1,
    pub cosmic: Option<ZcosmicToplevelHandleV1>,
    pub ident: Option<String>,
    pub app_id: String,
    pub title: String,
    pub states: BTreeSet<ToplevelState>,
    pub geometry: Option<GlobalRect>,
    pub workspaces: Vec<Key>,
    /// Whether the first `done` has been seen: before it the toplevel is half described.
    pub announced: bool,
}

/// Everything the handlers record.
#[derive(Debug)]
pub(crate) struct WlState {
    qh: QueueHandle<WlState>,
    connection: Connection,
    list: Option<ExtForeignToplevelListV1>,
    info: Option<ZcosmicToplevelInfoV1>,
    manager: Option<ExtWorkspaceManagerV1>,
    outputs: Vec<WlOutput>,
    pub toplevels: BTreeMap<Key, Toplevel>,
    pub workspaces: BTreeMap<Key, Workspace>,
    pub events: VecDeque<DesktopEvent>,
    issued: u64,
    done: u64,
}

impl WlState {
    /// A state that has asked for the registry.
    pub(crate) fn new(connection: &Connection, qh: QueueHandle<WlState>) -> Self {
        connection.display().get_registry(&qh, ());
        Self {
            qh,
            connection: connection.clone(),
            list: None,
            info: None,
            manager: None,
            outputs: Vec::new(),
            toplevels: BTreeMap::new(),
            workspaces: BTreeMap::new(),
            events: VecDeque::new(),
            issued: 0,
            done: 0,
        }
    }

    /// Asks for a round trip; the returned number is done once every request before it has been
    /// answered.
    pub(crate) fn sync(&mut self) -> u64 {
        self.issued += 1;
        self.connection.display().sync(&self.qh, SyncTag);
        self.issued
    }

    /// Whether round trip `n` has come back.
    pub(crate) fn synced(&self, n: u64) -> bool {
        self.done >= n
    }

    /// Whether the compositor offered the toplevel list at all.
    pub(crate) fn has_list(&self) -> bool {
        self.list.is_some()
    }

    fn adopt_cosmic(&mut self) {
        let Some(info) = self.info.clone() else {
            return;
        };
        for (key, top) in &mut self.toplevels {
            if top.cosmic.is_none() {
                top.cosmic = Some(info.get_cosmic_toplevel(&top.ext, &self.qh, *key));
            }
        }
    }

    /// The identifier of `key`, once it has one.
    fn ident_of(&self, key: Key) -> Option<ToplevelIdent> {
        let top = self.toplevels.get(&key)?;
        ToplevelIdent::parse(top.ident.as_deref()?).ok()
    }
}

fn states_of(bytes: &[u8]) -> BTreeSet<ToplevelState> {
    let (words, _) = bytes.as_chunks::<4>();
    words
        .iter()
        .filter_map(|w| match u32::from_ne_bytes(*w) {
            0 => Some(ToplevelState::Maximized),
            1 => Some(ToplevelState::Minimized),
            2 => Some(ToplevelState::Activated),
            _ => None,
        })
        .collect()
}

fn px(n: i32) -> Px {
    Px(u32::try_from(n).unwrap_or(0))
}

impl Dispatch<WlRegistry, ()> for WlState {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        else {
            return;
        };
        match interface.as_str() {
            "ext_foreign_toplevel_list_v1" => {
                state.list = Some(registry.bind(name, 1, qh, ()));
            }
            "zcosmic_toplevel_info_v1" if version >= INFO_VERSION => {
                state.info = Some(registry.bind(name, INFO_VERSION, qh, ()));
                state.adopt_cosmic();
            }
            "ext_workspace_manager_v1" => {
                state.manager = Some(registry.bind(name, 1, qh, ()));
            }
            "wl_output" => state.outputs.push(registry.bind(name, version.min(4), qh, ())),
            _ => {}
        }
    }
}

impl Dispatch<WlOutput, ()> for WlState {
    fn event(_: &mut Self, _: &WlOutput, _: <WlOutput as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WlCallback, SyncTag> for WlState {
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
        state.done += 1;
    }
}

impl Dispatch<ExtForeignToplevelListV1, ()> for WlState {
    fn event(
        state: &mut Self,
        _: &ExtForeignToplevelListV1,
        event: ext_foreign_toplevel_list_v1::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } = event else {
            return;
        };
        let Some(key) = toplevel.data::<Key>().copied() else {
            return;
        };
        let cosmic = state
            .info
            .as_ref()
            .map(|info| info.get_cosmic_toplevel(&toplevel, qh, key));
        state.toplevels.insert(
            key,
            Toplevel {
                ext: toplevel,
                cosmic,
                ident: None,
                app_id: String::new(),
                title: String::new(),
                states: BTreeSet::new(),
                geometry: None,
                workspaces: Vec::new(),
                announced: false,
            },
        );
    }

    event_created_child!(WlState, ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (ExtForeignToplevelHandleV1, Key::fresh()),
    ]);
}

impl Dispatch<ExtForeignToplevelHandleV1, Key> for WlState {
    fn event(
        state: &mut Self,
        handle: &ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        key: &Key,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_foreign_toplevel_handle_v1::Event;
        if let Event::Closed = event {
            let closed = state.ident_of(*key);
            let announced = state.toplevels.get(key).is_some_and(|t| t.announced);
            if let Some(top) = state.toplevels.remove(key)
                && let Some(cosmic) = top.cosmic
            {
                cosmic.destroy();
            }
            handle.destroy();
            if let (Some(ident), true) = (closed, announced) {
                state.events.push_back(DesktopEvent::WindowClosed(ident));
            }
            return;
        }
        let Some(top) = state.toplevels.get_mut(key) else {
            return;
        };
        match event {
            Event::Title { title } => top.title = title,
            Event::AppId { app_id } => top.app_id = app_id,
            Event::Identifier { identifier } => top.ident = Some(identifier),
            Event::Done => top.announced = true,
            _ => {}
        }
    }
}

impl Dispatch<ZcosmicToplevelInfoV1, ()> for WlState {
    fn event(
        _: &mut Self,
        _: &ZcosmicToplevelInfoV1,
        _: zcosmic_toplevel_info_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZcosmicToplevelHandleV1, Key> for WlState {
    fn event(
        state: &mut Self,
        _: &ZcosmicToplevelHandleV1,
        event: zcosmic_toplevel_handle_v1::Event,
        key: &Key,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zcosmic_toplevel_handle_v1::Event;
        let ident = state.ident_of(*key);
        let Some(top) = state.toplevels.get_mut(key) else {
            return;
        };
        match event {
            Event::State { state: bytes } => top.states = states_of(&bytes),
            Event::Geometry {
                x, y, width, height, ..
            } => {
                let rect = GlobalRect {
                    x: GlobalCoord(x),
                    y: GlobalCoord(y),
                    w: px(width),
                    h: px(height),
                };
                let moved = top.geometry.is_some_and(|old| old != rect);
                top.geometry = Some(rect);
                if let (true, true, Some(ident)) = (moved, top.announced, ident) {
                    state
                        .events
                        .push_back(DesktopEvent::GeometryChanged(ident, rect));
                }
            }
            Event::ExtWorkspaceEnter { workspace } => {
                if let Some(ws) = workspace.data::<Key>() {
                    top.workspaces.push(*ws);
                }
            }
            Event::ExtWorkspaceLeave { workspace } => {
                if let Some(ws) = workspace.data::<Key>() {
                    top.workspaces.retain(|have| have != ws);
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtWorkspaceManagerV1, ()> for WlState {
    fn event(
        _: &mut Self,
        _: &ExtWorkspaceManagerV1,
        _: ext_workspace_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }

    event_created_child!(WlState, ExtWorkspaceManagerV1, [
        ext_workspace_manager_v1::EVT_WORKSPACE_GROUP_OPCODE => (ExtWorkspaceGroupHandleV1, ()),
        ext_workspace_manager_v1::EVT_WORKSPACE_OPCODE => (ExtWorkspaceHandleV1, Key::fresh()),
    ]);
}

impl Dispatch<ExtWorkspaceGroupHandleV1, ()> for WlState {
    fn event(
        _: &mut Self,
        _: &ExtWorkspaceGroupHandleV1,
        _: ext_workspace_group_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtWorkspaceHandleV1, Key> for WlState {
    fn event(
        state: &mut Self,
        handle: &ExtWorkspaceHandleV1,
        event: ext_workspace_handle_v1::Event,
        key: &Key,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_workspace_handle_v1::Event;
        match event {
            Event::Id { id } => state.workspaces.entry(*key).or_default().id = id,
            Event::State { state: bits } => {
                let active = match bits {
                    WEnum::Value(bits) => bits.contains(WorkspaceBits::Active),
                    WEnum::Unknown(raw) => raw & WorkspaceBits::Active.bits() != 0,
                };
                state.workspaces.entry(*key).or_default().active = active;
            }
            Event::Removed => {
                state.workspaces.remove(key);
                handle.destroy();
            }
            _ => {}
        }
    }
}
