//! What the compositor has said so far: the toplevels (`ext_foreign_toplevel_list_v1` and the
//! COSMIC info beside it), the workspaces (`ext_workspace_v1`) and the events cuad has not
//! been told yet. The handlers only record; [`crate::wayland`] turns the record into answers.

use super::key::{ObjectKey as Key, Rounds, SyncTag};
use super::proto::client::zcosmic_toplevel_handle_v1::{self, ZcosmicToplevelHandleV1};
use super::proto::client::zcosmic_toplevel_info_v1::{self, ZcosmicToplevelInfoV1};
use crate::{DesktopEvent, GlobalCoord, GlobalRect, ToplevelIdent, ToplevelState};
use porter_core::Px;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::{Connection, Dispatch, QueueHandle, WEnum, event_created_child};
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

/// A workspace as `ext_workspace_v1` describes it.
#[derive(Debug, Default)]
pub(crate) struct Workspace {
    pub id: String,
    pub state: WorkspaceState,
}

/// Whether a workspace is the one on show.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum WorkspaceState {
    #[default]
    Inactive,
    Active,
}

/// Whether a toplevel has been fully described.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Announcement {
    /// Before the first `done`: the toplevel is half described.
    #[default]
    Pending,
    /// The first `done` has been seen.
    Announced,
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
    pub announcement: Announcement,
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
    rounds: Rounds,
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
            rounds: Rounds::default(),
        }
    }

    /// Asks for a round trip; the returned number is done once every request before it has been
    /// answered.
    pub(crate) fn sync(&mut self) -> u64 {
        self.rounds.ask(&self.connection, &self.qh)
    }

    /// Whether round trip `n` has come back.
    pub(crate) fn synced(&self, n: u64) -> bool {
        self.rounds.reached(n)
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
            "wl_output" => state
                .outputs
                .push(registry.bind(name, version.min(4), qh, ())),
            _ => {}
        }
    }
}

impl Dispatch<WlOutput, ()> for WlState {
    fn event(
        _: &mut Self,
        _: &WlOutput,
        _: <WlOutput as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
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
        state.rounds.answered();
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
        let key = Key::of(&toplevel);
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
                announcement: Announcement::Pending,
            },
        );
    }

    event_created_child!(WlState, ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (ExtForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ExtForeignToplevelHandleV1, ()> for WlState {
    fn event(
        state: &mut Self,
        handle: &ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_foreign_toplevel_handle_v1::Event;
        let key = Key::of(handle);
        if let Event::Closed = event {
            let closed = state.ident_of(key);
            let announced = state
                .toplevels
                .get(&key)
                .is_some_and(|t| t.announcement == Announcement::Announced);
            if let Some(top) = state.toplevels.remove(&key)
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
        let Some(top) = state.toplevels.get_mut(&key) else {
            return;
        };
        match event {
            Event::Title { title } => top.title = title,
            Event::AppId { app_id } => top.app_id = app_id,
            Event::Identifier { identifier } => top.ident = Some(identifier),
            Event::Done => top.announcement = Announcement::Announced,
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
                x,
                y,
                width,
                height,
                ..
            } => {
                let rect = GlobalRect {
                    x: GlobalCoord(x),
                    y: GlobalCoord(y),
                    w: px(width),
                    h: px(height),
                };
                let moved = top.geometry.is_some_and(|old| old != rect);
                top.geometry = Some(rect);
                if let (true, Announcement::Announced, Some(ident)) =
                    (moved, top.announcement, ident)
                {
                    state
                        .events
                        .push_back(DesktopEvent::GeometryChanged(ident, rect));
                }
            }
            Event::ExtWorkspaceEnter { workspace } => {
                top.workspaces.push(Key::of(&workspace));
            }
            Event::ExtWorkspaceLeave { workspace } => {
                let ws = Key::of(&workspace);
                top.workspaces.retain(|have| *have != ws);
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
        ext_workspace_manager_v1::EVT_WORKSPACE_OPCODE => (ExtWorkspaceHandleV1, ()),
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

impl Dispatch<ExtWorkspaceHandleV1, ()> for WlState {
    fn event(
        state: &mut Self,
        handle: &ExtWorkspaceHandleV1,
        event: ext_workspace_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_workspace_handle_v1::Event;
        let key = &Key::of(handle);
        match event {
            Event::Id { id } => state.workspaces.entry(*key).or_default().id = id,
            Event::State { state: bits } => {
                let active = match bits {
                    WEnum::Value(bits) => bits.contains(WorkspaceBits::Active),
                    WEnum::Unknown(raw) => raw & WorkspaceBits::Active.bits() != 0,
                };
                state.workspaces.entry(*key).or_default().state = if active {
                    WorkspaceState::Active
                } else {
                    WorkspaceState::Inactive
                };
            }
            Event::Removed => {
                state.workspaces.remove(key);
                handle.destroy();
            }
            _ => {}
        }
    }
}
