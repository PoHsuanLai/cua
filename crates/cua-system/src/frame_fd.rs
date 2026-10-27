//! A frame's bytes as a sealed memfd: what rides beside a `CuaStep` on the inferd session's fd.

use cua_run::FrameAttachment;
use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::os::fd::OwnedFd;

/// Why the frame could not be made into a descriptor.
#[derive(Debug, thiserror::Error)]
#[error("cannot hand the frame over as a memfd: {0}")]
pub struct FrameFdError(#[from] std::io::Error);

/// The frame's pixels in a new memfd: written whole, the offset put back at the start (a
/// descriptor sent with SCM_RIGHTS shares its offset with the receiver, who may `read` it), and
/// sealed against writes and resizing so inferd can map it without a copy. The bytes never pass
/// through JSON or the bus.
pub fn frame_fd(frame: &FrameAttachment) -> Result<OwnedFd, FrameFdError> {
    let fd = memfd_create(
        "cuad-frame",
        MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
    )
    .map_err(std::io::Error::from)?;
    let mut file = File::from(fd);
    file.write_all(&frame.0.data)?;
    file.seek(SeekFrom::Start(0))?;
    let fd = OwnedFd::from(file);
    fcntl_add_seals(
        &fd,
        SealFlags::WRITE | SealFlags::SHRINK | SealFlags::GROW | SealFlags::SEAL,
    )
    .map_err(std::io::Error::from)?;
    Ok(fd)
}
