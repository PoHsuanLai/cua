//! The fake compositor's state and its dispatch.

use super::cosmic_server::zcosmic_toplevel_handle_v1::{self, ZcosmicToplevelHandleV1};
use super::cosmic_server::zcosmic_toplevel_info_v1::{self, ZcosmicToplevelInfoV1};
use rustix::event::{PollFd, PollFlags, poll};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use wayland_protocols::ext::foreign_toplevel_list::v1::server::ext_foreign_toplevel_handle_v1::{
    self, ExtForeignToplevelHandleV1,
};
use wayland_protocols::ext::foreign_toplevel_list::v1::server::ext_foreign_toplevel_list_v1::{
    self, ExtForeignToplevelListV1,
};
use wayland_protocols::ext::workspace::v1::server::ext_workspace_group_handle_v1::{
    self, ExtWorkspaceGroupHandleV1,
};
use wayland_protocols::ext::workspace::v1::server::ext_workspace_handle_v1::{
    self, ExtWorkspaceHandleV1, State as WorkspaceBits,
};
use wayland_protocols::ext::workspace::v1::server::ext_workspace_manager_v1::{
    self, ExtWorkspaceManagerV1,
};
use wayland_server::backend::ClientData;
use wayland_server::protocol::wl_output::{self, WlOutput};
use wayland_server::{
    Client, DataInit, Dispatch, Display, DisplayHandle, GlobalDispatch, New, Resource,
};

/// A toplevel the test made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopId(pub usize);

/// A workspace the test made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WsId(pub usize);

/// What a new toplevel looks like.
#[derive(Debug, Clone)]
pub struct Spec {
    pub identifier: &'static str,
    pub app_id: &'static str,
    pub title: &'static str,
    /// `zcosmic_toplevel_handle_v1.state` values: 0 maximized, 1 minimized, 2 activated.
    pub states: Vec<u32>,
    pub geometry: (i32, i32, i32, i32),
    pub workspace: Option<WsId>,
}

struct Top {
    spec: Spec,
    handles: Vec<ExtForeignToplevelHandleV1>,
    cosmic: Vec<ZcosmicToplevelHandleV1>,
    closed: bool,
}

struct Ws {
    id: &'static str,
    active: bool,
    handles: Vec<ExtWorkspaceHandleV1>,
}

/// Which globals the compositor offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offer {
    /// Everything stock cosmic-comp offers.
    Cosmic,
    /// The toplevel list alone (no cosmic info, no workspaces).
    ListOnly,
    /// Neither.
    Nothing,
}

pub struct Compositor {
    dh: DisplayHandle,
    outputs: Vec<WlOutput>,
    lists: Vec<ExtForeignToplevelListV1>,
    tops: Vec<Top>,
    wss: Vec<Ws>,
    clients: Vec<Client>,
    managers: Vec<ExtWorkspaceManagerV1>,
}

struct Anyone;
impl ClientData for Anyone {}

enum Job {
    Run(Box<dyn FnOnce(&mut Compositor) + Send>),
    Quit,
}

/// The running fake: a thread, a doorbell and the client's end of the socket.
pub struct FakeCompositor {
    jobs: Sender<Job>,
    bell: UnixStream,
    thread: Option<JoinHandle<()>>,
    client: Option<UnixStream>,
}

impl FakeCompositor {
    /// Starts a compositor offering `offer`; the client's socket is taken with [`Self::client`].
    pub fn start(offer: Offer) -> Self {
        let (client, server) = UnixStream::pair().expect("socketpair");
        let (bell, ring) = UnixStream::pair().expect("doorbell");
        ring.set_nonblocking(true).expect("nonblocking");
        let (jobs, queue) = channel();
        let display = Display::<Compositor>::new().expect("display");
        let mut dh = display.handle();
        if offer != Offer::Nothing {
            dh.create_global::<Compositor, ExtForeignToplevelListV1, _>(1, ());
            dh.create_global::<Compositor, WlOutput, _>(4, ());
        }
        if offer == Offer::Cosmic {
            dh.create_global::<Compositor, ZcosmicToplevelInfoV1, _>(3, ());
            dh.create_global::<Compositor, ExtWorkspaceManagerV1, _>(1, ());
        }
        let client_handle = dh.insert_client(server, Arc::new(Anyone)).expect("client");
        let state = Compositor {
            dh,
            outputs: Vec::new(),
            lists: Vec::new(),
            tops: Vec::new(),
            wss: Vec::new(),
            clients: vec![client_handle],
            managers: Vec::new(),
        };
        let thread = std::thread::spawn(move || serve(display, state, queue, ring));
        Self {
            jobs,
            bell,
            thread: Some(thread),
            client: Some(client),
        }
    }

    /// The client's end of the socket (once).
    pub fn client(&mut self) -> UnixStream {
        self.client.take().expect("the client socket is taken once")
    }

    /// Runs `f` on the compositor's thread and returns what it returns.
    pub fn call<R: Send + 'static>(&self, f: impl FnOnce(&mut Compositor) -> R + Send + 'static) -> R {
        let (reply, answer) = channel();
        self.send(Job::Run(Box::new(move |c| {
            let _ = reply.send(f(c));
        })));
        answer.recv().expect("the compositor answers")
    }

    fn send(&self, job: Job) {
        use std::io::Write;
        self.jobs.send(job).expect("the compositor runs");
        (&self.bell).write_all(&[1]).expect("ring");
    }

    /// Ends the compositor: the client sees its connection close.
    pub fn shutdown(&mut self) {
        if let Some(thread) = self.thread.take() {
            self.send(Job::Quit);
            thread.join().expect("the compositor thread");
        }
    }

    /// A workspace.
    pub fn workspace(&self, id: &'static str, active: bool) -> WsId {
        self.call(move |c| {
            c.wss.push(Ws { id, active, handles: Vec::new() });
            WsId(c.wss.len() - 1)
        })
    }

    /// A toplevel, announced to every list that is bound.
    pub fn add(&self, spec: Spec) -> TopId {
        self.call(move |c| c.add(spec))
    }

    /// The toplevel's geometry changes.
    pub fn move_to(&self, top: TopId, geometry: (i32, i32, i32, i32)) {
        self.call(move |c| c.move_to(top, geometry));
    }

    /// The toplevel closes.
    pub fn close(&self, top: TopId) {
        self.call(move |c| c.close(top));
    }

    /// A workspace turns active or inactive.
    pub fn show(&self, ws: WsId, active: bool) {
        self.call(move |c| c.show(ws, active));
    }
}

impl Drop for FakeCompositor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn serve(
    mut display: Display<Compositor>,
    mut state: Compositor,
    queue: Receiver<Job>,
    ring: UnixStream,
) {
    use std::io::Read;
    loop {
        let ready = {
            let mut fds = [
                PollFd::new(&display, PollFlags::IN),
                PollFd::new(&ring, PollFlags::IN),
            ];
            loop {
                match poll(&mut fds, None) {
                    Ok(_) => break,
                    Err(rustix::io::Errno::INTR) => {}
                    Err(error) => panic!("poll: {error}"),
                }
            }
            fds[1].revents().contains(PollFlags::IN)
        };
        if ready {
            let mut drain = [0u8; 64];
            let _ = (&ring).read(&mut drain);
            while let Ok(job) = queue.try_recv() {
                match job {
                    Job::Run(run) => run(&mut state),
                    Job::Quit => return,
                }
            }
        }
        display.dispatch_clients(&mut state).expect("dispatch");
        display.flush_clients().expect("flush");
    }
}

impl Compositor {
    fn announce(&mut self, list: &ExtForeignToplevelListV1, index: usize) {
        let Some(client) = list.client() else { return };
        let Ok(handle) = client.create_resource::<ExtForeignToplevelHandleV1, TopId, Compositor>(
            &self.dh,
            list.version(),
            TopId(index),
        ) else {
            return;
        };
        list.toplevel(&handle);
        let spec = &self.tops[index].spec;
        handle.identifier(spec.identifier.into());
        handle.app_id(spec.app_id.into());
        handle.title(spec.title.into());
        handle.done();
        self.tops[index].handles.push(handle);
    }

    fn add(&mut self, spec: Spec) -> TopId {
        self.tops.push(Top {
            spec,
            handles: Vec::new(),
            cosmic: Vec::new(),
            closed: false,
        });
        let index = self.tops.len() - 1;
        for list in self.lists.clone() {
            self.announce(&list, index);
        }
        TopId(index)
    }

    fn cosmic_events(&self, cosmic: &ZcosmicToplevelHandleV1, index: usize) {
        let top = &self.tops[index];
        let bytes: Vec<u8> = top
            .spec
            .states
            .iter()
            .flat_map(|s| s.to_ne_bytes())
            .collect();
        cosmic.state(bytes);
        if let Some(out) = self.outputs.first() {
            let (x, y, w, h) = top.spec.geometry;
            cosmic.geometry(out, x, y, w, h);
        }
        if let Some(ws) = top.spec.workspace {
            for handle in &self.wss[ws.0].handles {
                cosmic.ext_workspace_enter(handle);
            }
        }
    }

    fn move_to(&mut self, top: TopId, geometry: (i32, i32, i32, i32)) {
        self.tops[top.0].spec.geometry = geometry;
        let (x, y, w, h) = geometry;
        if let Some(out) = self.outputs.first() {
            for cosmic in &self.tops[top.0].cosmic {
                cosmic.geometry(out, x, y, w, h);
            }
        }
    }

    fn close(&mut self, top: TopId) {
        self.tops[top.0].closed = true;
        for handle in &self.tops[top.0].handles {
            handle.closed();
        }
    }

    fn show(&mut self, ws: WsId, active: bool) {
        self.wss[ws.0].active = active;
        let bits = if active {
            WorkspaceBits::Active
        } else {
            WorkspaceBits::empty()
        };
        for handle in &self.wss[ws.0].handles {
            handle.state(bits);
        }
        for manager in &self.managers {
            manager.done();
        }
    }
}

impl GlobalDispatch<ExtForeignToplevelListV1, ()> for Compositor {
    fn bind(
        state: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<ExtForeignToplevelListV1>,
        _: &(),
        init: &mut DataInit<'_, Self>,
    ) {
        let list = init.init(resource, ());
        state.lists.push(list.clone());
        for index in 0..state.tops.len() {
            if !state.tops[index].closed {
                state.announce(&list, index);
            }
        }
    }
}

impl Dispatch<ExtForeignToplevelListV1, ()> for Compositor {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &ExtForeignToplevelListV1,
        _: ext_foreign_toplevel_list_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl Dispatch<ExtForeignToplevelHandleV1, TopId> for Compositor {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &ExtForeignToplevelHandleV1,
        _: ext_foreign_toplevel_handle_v1::Request,
        _: &TopId,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl GlobalDispatch<WlOutput, ()> for Compositor {
    fn bind(
        state: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<WlOutput>,
        _: &(),
        init: &mut DataInit<'_, Self>,
    ) {
        state.outputs.push(init.init(resource, ()));
    }
}

impl Dispatch<WlOutput, ()> for Compositor {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &WlOutput,
        _: wl_output::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl GlobalDispatch<ZcosmicToplevelInfoV1, ()> for Compositor {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<ZcosmicToplevelInfoV1>,
        _: &(),
        init: &mut DataInit<'_, Self>,
    ) {
        init.init(resource, ());
    }
}

impl Dispatch<ZcosmicToplevelInfoV1, ()> for Compositor {
    fn request(
        state: &mut Self,
        _: &Client,
        info: &ZcosmicToplevelInfoV1,
        request: zcosmic_toplevel_info_v1::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        let zcosmic_toplevel_info_v1::Request::GetCosmicToplevel {
            cosmic_toplevel,
            foreign_toplevel,
        } = request
        else {
            return;
        };
        let Some(TopId(index)) = foreign_toplevel.data::<TopId>().copied() else {
            return;
        };
        let cosmic = init.init(cosmic_toplevel, TopId(index));
        state.cosmic_events(&cosmic, index);
        state.tops[index].cosmic.push(cosmic);
        info.done();
    }
}

impl Dispatch<ZcosmicToplevelHandleV1, TopId> for Compositor {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &ZcosmicToplevelHandleV1,
        _: zcosmic_toplevel_handle_v1::Request,
        _: &TopId,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl GlobalDispatch<ExtWorkspaceManagerV1, ()> for Compositor {
    fn bind(
        state: &mut Self,
        dh: &DisplayHandle,
        client: &Client,
        resource: New<ExtWorkspaceManagerV1>,
        _: &(),
        init: &mut DataInit<'_, Self>,
    ) {
        let manager = init.init(resource, ());
        let version = manager.version();
        let group = client
            .create_resource::<ExtWorkspaceGroupHandleV1, (), Compositor>(dh, version, ())
            .expect("group");
        manager.workspace_group(&group);
        for index in 0..state.wss.len() {
            let handle = client
                .create_resource::<ExtWorkspaceHandleV1, WsId, Compositor>(dh, version, WsId(index))
                .expect("workspace");
            manager.workspace(&handle);
            handle.id(state.wss[index].id.into());
            handle.state(if state.wss[index].active {
                WorkspaceBits::Active
            } else {
                WorkspaceBits::empty()
            });
            group.workspace_enter(&handle);
            state.wss[index].handles.push(handle);
        }
        manager.done();
        state.managers.push(manager);
    }
}

impl Dispatch<ExtWorkspaceManagerV1, ()> for Compositor {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &ExtWorkspaceManagerV1,
        _: ext_workspace_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl Dispatch<ExtWorkspaceGroupHandleV1, ()> for Compositor {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &ExtWorkspaceGroupHandleV1,
        _: ext_workspace_group_handle_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl Dispatch<ExtWorkspaceHandleV1, WsId> for Compositor {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &ExtWorkspaceHandleV1,
        _: ext_workspace_handle_v1::Request,
        _: &WsId,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}
