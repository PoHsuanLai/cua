//! The fake compositor's state and its dispatch.

mod agent;
mod dispatch;

pub use agent::Stance;

use rustix::event::{PollFd, PollFlags, poll};
use rustix::io::pwrite;
use std::collections::BTreeSet;
use std::os::fd::OwnedFd;
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
use wayland_protocols::ext::image_capture_source::v1::server::ext_foreign_toplevel_image_capture_source_manager_v1::{
    self, ExtForeignToplevelImageCaptureSourceManagerV1,
};
use wayland_protocols::ext::image_capture_source::v1::server::ext_image_capture_source_v1::{
    self, ExtImageCaptureSourceV1,
};
use wayland_protocols::ext::image_copy_capture::v1::server::ext_image_copy_capture_frame_v1::{
    self, ExtImageCopyCaptureFrameV1, FailureReason,
};
use wayland_protocols::ext::image_copy_capture::v1::server::ext_image_copy_capture_manager_v1::{
    self, ExtImageCopyCaptureManagerV1,
};
use wayland_protocols::ext::image_copy_capture::v1::server::ext_image_copy_capture_session_v1::{
    self, ExtImageCopyCaptureSessionV1,
};
use casement_protocols::server::quire_agent_v1::quire_agent_manager_v1::QuireAgentManagerV1;
use wayland_server::backend::ClientData;
use wayland_server::protocol::wl_buffer::{self, WlBuffer};
use wayland_server::protocol::wl_shm::{self, Format, WlShm};
use wayland_server::protocol::wl_shm_pool::{self, WlShmPool};
use wayland_server::{Client, DataInit, Dispatch, Display, DisplayHandle, GlobalDispatch, New, Resource};

/// What a toplevel shows: its size and its pixels (4 bytes each, `size.0 * size.1 * 4` of them).
#[derive(Debug, Clone)]
pub struct Window {
    pub identifier: &'static str,
    pub size: (u32, u32),
    pub pixels: Vec<u8>,
}

impl Window {
    /// A window filled with one colour.
    pub fn solid(identifier: &'static str, size: (u32, u32), rgba: [u8; 4]) -> Self {
        let pixels = rgba
            .iter()
            .copied()
            .cycle()
            .take((size.0 * size.1 * 4) as usize)
            .collect();
        Self {
            identifier,
            size,
            pixels,
        }
    }
}

struct Pool {
    file: Arc<OwnedFd>,
}

struct Buf {
    file: Arc<OwnedFd>,
    offset: i32,
    size: (i32, i32),
    stride: i32,
}

struct FrameData {
    session: usize,
    buffer: std::sync::Mutex<Option<WlBuffer>>,
}

struct SessionData {
    toplevel: usize,
}

pub struct Compositor {
    dh: DisplayHandle,
    windows: Vec<Window>,
    closed: Vec<bool>,
    handles: Vec<Vec<ExtForeignToplevelHandleV1>>,
    sessions: Vec<ExtImageCopyCaptureSessionV1>,
    session_toplevel: Vec<usize>,
    offered: Vec<Format>,
    pub frames_served: usize,
    stance: Stance,
    hints: Vec<Vec<u8>>,
    damage: Damage,
    /// The sessions that have been given a frame.
    served: BTreeSet<usize>,
    /// Frames kept back until the window shows something new.
    held: Vec<ExtImageCopyCaptureFrameV1>,
}

/// When the compositor completes a frame.
#[derive(Debug, Clone, Copy)]
pub enum Damage {
    /// At once, with full damage (a stock compositor).
    Always,
    /// The first frame of a session at once, the next only after the window shows new pixels
    /// (ext-image-copy-capture on damage).
    OnChange,
}

struct Anyone;
impl ClientData for Anyone {}

enum Job {
    Run(Box<dyn FnOnce(&mut Compositor) + Send>),
    Quit,
}

pub struct FakeCompositor {
    jobs: Sender<Job>,
    bell: UnixStream,
    thread: Option<JoinHandle<()>>,
    client: Option<UnixStream>,
}

impl FakeCompositor {
    /// A compositor offering the capture globals, with `windows` already on screen.
    pub fn start(windows: Vec<Window>) -> Self {
        Self::start_with(windows, None)
    }

    /// The same, also offering `quire_agent_manager_v1`, answering leases as `stance` says.
    pub fn start_with_agent(windows: Vec<Window>, stance: Stance) -> Self {
        Self::start_with(windows, Some(stance))
    }

    fn start_with(windows: Vec<Window>, agent: Option<Stance>) -> Self {
        let (client, server) = UnixStream::pair().expect("socketpair");
        let (bell, ring) = UnixStream::pair().expect("doorbell");
        ring.set_nonblocking(true).expect("nonblocking");
        let (jobs, queue) = channel();
        let display = Display::<Compositor>::new().expect("display");
        let mut dh = display.handle();
        dh.create_global::<Compositor, ExtForeignToplevelListV1, _>(1, ());
        dh.create_global::<Compositor, ExtForeignToplevelImageCaptureSourceManagerV1, _>(1, ());
        dh.create_global::<Compositor, ExtImageCopyCaptureManagerV1, _>(1, ());
        dh.create_global::<Compositor, WlShm, _>(1, ());
        if agent.is_some() {
            dh.create_global::<Compositor, QuireAgentManagerV1, _>(1, ());
        }
        dh.insert_client(server, Arc::new(Anyone)).expect("client");
        let n = windows.len();
        let state = Compositor {
            dh,
            windows,
            closed: vec![false; n],
            handles: vec![Vec::new(); n],
            sessions: Vec::new(),
            session_toplevel: Vec::new(),
            // What a stock compositor offers: the 10-bit pair first, then the 8-bit pair.
            offered: vec![
                Format::Abgr2101010,
                Format::Xbgr2101010,
                Format::Xbgr8888,
                Format::Abgr8888,
            ],
            frames_served: 0,
            stance: agent.unwrap_or(Stance::Grant),
            hints: Vec::new(),
            damage: Damage::Always,
            served: BTreeSet::new(),
            held: Vec::new(),
        };
        let thread = std::thread::spawn(move || serve(display, state, queue, ring));
        Self {
            jobs,
            bell,
            thread: Some(thread),
            client: Some(client),
        }
    }

    pub fn client(&mut self) -> UnixStream {
        self.client.take().expect("the client socket is taken once")
    }

    pub fn call<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Compositor) -> R + Send + 'static,
    ) -> R {
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

    pub fn shutdown(&mut self) {
        if let Some(thread) = self.thread.take() {
            self.send(Job::Quit);
            thread.join().expect("the compositor thread");
        }
    }

    /// What window `index` shows from the next frame on.
    pub fn show(&self, index: usize, pixels: Vec<u8>) {
        self.call(move |c| {
            c.windows[index].pixels = pixels;
            for frame in std::mem::take(&mut c.held) {
                if let Some(data) = frame.data::<FrameData>() {
                    c.serve_frame(&frame, data);
                }
            }
        });
    }

    /// From now on a session gets its first frame at once and later ones only after `show`.
    pub fn damage_only_on_change(&self) {
        self.call(|c| c.damage = Damage::OnChange);
    }

    /// The window is resized: every session is told its new constraints.
    pub fn resize(&self, index: usize, size: (u32, u32), pixels: Vec<u8>) {
        self.call(move |c| c.resize(index, size, pixels));
    }

    /// The window closes: its sessions stop.
    pub fn close(&self, index: usize) {
        self.call(move |c| c.close(index));
    }

    /// Only these shm formats are offered from now on (to sessions made later).
    pub fn offer(&self, formats: Vec<Format>) {
        self.call(move |c| c.offered = formats);
    }

    /// The `set_mask_hints` arrays received so far, in order.
    pub fn hints(&self) -> Vec<Vec<u8>> {
        self.call(|c| c.hints.clone())
    }

    /// How many frames have been served.
    pub fn frames_served(&self) -> usize {
        self.call(|c| c.frames_served)
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
    fn constraints(&self, session: &ExtImageCopyCaptureSessionV1, toplevel: usize) {
        let (w, h) = self.windows[toplevel].size;
        session.buffer_size(w, h);
        for format in &self.offered {
            session.shm_format(*format);
        }
        session.done();
    }

    fn resize(&mut self, index: usize, size: (u32, u32), pixels: Vec<u8>) {
        self.windows[index].size = size;
        self.windows[index].pixels = pixels;
        for (session, top) in self.sessions.iter().zip(&self.session_toplevel) {
            if *top == index {
                self.constraints(session, index);
            }
        }
    }

    fn close(&mut self, index: usize) {
        self.closed[index] = true;
        for handle in &self.handles[index] {
            handle.closed();
        }
        for (session, top) in self.sessions.iter().zip(&self.session_toplevel) {
            if *top == index {
                session.stopped();
            }
        }
    }

    fn capture(&mut self, frame: &ExtImageCopyCaptureFrameV1, data: &FrameData) {
        let first = self.served.insert(data.session);
        match (self.damage, first) {
            (Damage::OnChange, false) => self.held.push(frame.clone()),
            _ => self.serve_frame(frame, data),
        }
    }

    fn serve_frame(&mut self, frame: &ExtImageCopyCaptureFrameV1, data: &FrameData) {
        let top = self.session_toplevel[data.session];
        let window = &self.windows[top];
        let buffer = data.buffer.lock().expect("frame lock").take();
        let Some(buffer) = buffer else {
            frame.failed(FailureReason::Unknown);
            return;
        };
        let Some(buf) = buffer.data::<Buf>() else {
            frame.failed(FailureReason::Unknown);
            return;
        };
        let (w, h) = window.size;
        if buf.size != (w as i32, h as i32) || self.closed[top] {
            frame.failed(if self.closed[top] {
                FailureReason::Stopped
            } else {
                FailureReason::BufferConstraints
            });
            return;
        }
        let mut done = 0;
        while done < window.pixels.len() {
            let at = buf.offset as u64 + done as u64;
            done += pwrite(&*buf.file, &window.pixels[done..], at).expect("pwrite");
        }
        let _ = buf.stride;
        frame.damage(0, 0, w as i32, h as i32);
        frame.ready();
        self.frames_served += 1;
    }
}
