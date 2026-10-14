//! The fake compositor's state and its dispatch.

use rustix::event::{PollFd, PollFlags, poll};
use rustix::io::pwrite;
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
        self.call(move |c| c.windows[index].pixels = pixels);
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

impl GlobalDispatch<ExtForeignToplevelListV1, ()> for Compositor {
    fn bind(
        state: &mut Self,
        dh: &DisplayHandle,
        client: &Client,
        resource: New<ExtForeignToplevelListV1>,
        _: &(),
        init: &mut DataInit<'_, Self>,
    ) {
        let list = init.init(resource, ());
        for index in 0..state.windows.len() {
            if state.closed[index] {
                continue;
            }
            let handle = client
                .create_resource::<ExtForeignToplevelHandleV1, usize, Compositor>(
                    dh,
                    list.version(),
                    index,
                )
                .expect("handle");
            list.toplevel(&handle);
            handle.identifier(state.windows[index].identifier.into());
            handle.done();
            state.handles[index].push(handle);
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

impl Dispatch<ExtForeignToplevelHandleV1, usize> for Compositor {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &ExtForeignToplevelHandleV1,
        _: ext_foreign_toplevel_handle_v1::Request,
        _: &usize,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl GlobalDispatch<ExtForeignToplevelImageCaptureSourceManagerV1, ()> for Compositor {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<ExtForeignToplevelImageCaptureSourceManagerV1>,
        _: &(),
        init: &mut DataInit<'_, Self>,
    ) {
        init.init(resource, ());
    }
}

impl Dispatch<ExtForeignToplevelImageCaptureSourceManagerV1, ()> for Compositor {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &ExtForeignToplevelImageCaptureSourceManagerV1,
        request: ext_foreign_toplevel_image_capture_source_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let ext_foreign_toplevel_image_capture_source_manager_v1::Request::CreateSource {
            source,
            toplevel_handle,
        } = request
        {
            let top = toplevel_handle
                .data::<usize>()
                .copied()
                .unwrap_or(usize::MAX);
            init.init(source, top);
        }
    }
}

impl Dispatch<ExtImageCaptureSourceV1, usize> for Compositor {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &ExtImageCaptureSourceV1,
        _: ext_image_capture_source_v1::Request,
        _: &usize,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl GlobalDispatch<ExtImageCopyCaptureManagerV1, ()> for Compositor {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<ExtImageCopyCaptureManagerV1>,
        _: &(),
        init: &mut DataInit<'_, Self>,
    ) {
        init.init(resource, ());
    }
}

impl Dispatch<ExtImageCopyCaptureManagerV1, ()> for Compositor {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &ExtImageCopyCaptureManagerV1,
        request: ext_image_copy_capture_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let ext_image_copy_capture_manager_v1::Request::CreateSession {
            session, source, ..
        } = request
        {
            let top = source.data::<usize>().copied().unwrap_or(usize::MAX);
            let resource = init.init(session, SessionData { toplevel: top });
            state.constraints(&resource, top);
            state.sessions.push(resource);
            state.session_toplevel.push(top);
        }
    }
}

impl Dispatch<ExtImageCopyCaptureSessionV1, SessionData> for Compositor {
    fn request(
        state: &mut Self,
        _: &Client,
        session: &ExtImageCopyCaptureSessionV1,
        request: ext_image_copy_capture_session_v1::Request,
        data: &SessionData,
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let ext_image_copy_capture_session_v1::Request::CreateFrame { frame } = request {
            let index = state
                .sessions
                .iter()
                .position(|s| s.id() == session.id())
                .unwrap_or(0);
            let _ = data.toplevel;
            init.init(
                frame,
                FrameData {
                    session: index,
                    buffer: std::sync::Mutex::new(None),
                },
            );
        }
    }
}

impl Dispatch<ExtImageCopyCaptureFrameV1, FrameData> for Compositor {
    fn request(
        state: &mut Self,
        _: &Client,
        frame: &ExtImageCopyCaptureFrameV1,
        request: ext_image_copy_capture_frame_v1::Request,
        data: &FrameData,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        use ext_image_copy_capture_frame_v1::Request;
        match request {
            Request::AttachBuffer { buffer } => {
                *data.buffer.lock().expect("frame lock") = Some(buffer)
            }
            Request::Capture => state.serve_frame(frame, data),
            _ => {}
        }
    }
}

impl GlobalDispatch<WlShm, ()> for Compositor {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<WlShm>,
        _: &(),
        init: &mut DataInit<'_, Self>,
    ) {
        init.init(resource, ());
    }
}

impl Dispatch<WlShm, ()> for Compositor {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &WlShm,
        request: wl_shm::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let wl_shm::Request::CreatePool { id, fd, .. } = request {
            init.init(id, Pool { file: Arc::new(fd) });
        }
    }
}

impl Dispatch<WlShmPool, Pool> for Compositor {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &WlShmPool,
        request: wl_shm_pool::Request,
        pool: &Pool,
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let wl_shm_pool::Request::CreateBuffer {
            id,
            offset,
            width,
            height,
            stride,
            ..
        } = request
        {
            init.init(
                id,
                Buf {
                    file: pool.file.clone(),
                    offset,
                    size: (width, height),
                    stride,
                },
            );
        }
    }
}

impl Dispatch<WlBuffer, Buf> for Compositor {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &WlBuffer,
        _: wl_buffer::Request,
        _: &Buf,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}
