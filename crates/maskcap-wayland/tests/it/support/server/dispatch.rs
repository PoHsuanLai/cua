//! The compositor's protocol handlers: what it answers to each request.

use super::*;

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
            Request::Capture => state.capture(frame, data),
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
