//! The buffer a frame is captured into: a memfd the compositor maps, read back with `pread` (no
//! mapping on our side, so no `unsafe`).

use super::state::CapState;
use crate::CaptureError;
use cua_action::{DeviceSize, PixelFormat};
use rustix::fs::{MemfdFlags, ftruncate, memfd_create};
use rustix::io::pread;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_shm::Format;
use wayland_client::protocol::wl_shm_pool::WlShmPool;

/// Bytes per pixel of the 8-bit formats.
const BPP: u32 = 4;

/// A pool and the one buffer in it, sized for a session's constraints at `epoch`.
#[derive(Debug)]
pub(crate) struct Shared {
    pool: WlShmPool,
    pub buffer: WlBuffer,
    file: OwnedFd,
    pub size: (u32, u32),
    pub epoch: u32,
    pub format: PixelFormat,
}

fn io(error: rustix::io::Errno) -> CaptureError {
    CaptureError::Protocol(format!("shared memory: {error}"))
}

impl Shared {
    pub(crate) fn make(
        state: &mut CapState,
        size: (u32, u32),
        epoch: u32,
        wire: Format,
        format: PixelFormat,
    ) -> Result<Self, CaptureError> {
        let stride = size.0.checked_mul(BPP);
        let bytes = stride.and_then(|s| s.checked_mul(size.1));
        let (Some(stride), Some(bytes)) = (stride, bytes) else {
            return Err(CaptureError::Format);
        };
        let (Ok(stride_i), Ok(w), Ok(h), Ok(bytes_i)) = (
            i32::try_from(stride),
            i32::try_from(size.0),
            i32::try_from(size.1),
            i32::try_from(bytes),
        ) else {
            return Err(CaptureError::Format);
        };
        if bytes == 0 {
            return Err(CaptureError::Format);
        }
        let file = memfd_create("maskcap-frame", MemfdFlags::CLOEXEC).map_err(io)?;
        ftruncate(&file, u64::from(bytes)).map_err(io)?;
        let qh = state.qh().clone();
        let pool = state
            .shm()?
            .create_pool(rustix::fd::AsFd::as_fd(&file), bytes_i, &qh, ());
        let buffer = pool.create_buffer(0, w, h, stride_i, wire, &qh, ());
        Ok(Self {
            pool,
            buffer,
            file,
            size,
            epoch,
            format,
        })
    }

    /// The pixels the compositor wrote.
    pub(crate) fn read(&self) -> Result<crate::PixelBuf, CaptureError> {
        let stride = self.size.0 * BPP;
        let len = usize::try_from(u64::from(stride) * u64::from(self.size.1))
            .map_err(|_| CaptureError::Format)?;
        let mut data = vec![0u8; len];
        let mut filled = 0;
        while filled < len {
            let n = pread(&self.file, &mut data[filled..], filled as u64).map_err(io)?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        if filled < len {
            return Err(CaptureError::Protocol(
                "short read of the frame buffer".into(),
            ));
        }
        Ok(crate::PixelBuf {
            format: self.format,
            size: DeviceSize {
                w: self.size.0,
                h: self.size.1,
            },
            stride,
            data: Arc::from(data),
        })
    }

    pub(crate) fn destroy(&self) {
        self.buffer.destroy();
        self.pool.destroy();
    }
}
