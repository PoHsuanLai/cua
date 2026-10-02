//! Bindings for `protocols/quire-agent-v1.xml`.
//!
//! The XML is the interface; this crate only runs wayland-scanner over it. The protocol builds on
//! `ext_foreign_toplevel_list_v1` (the toplevel a lease names) and `ext_image_capture_source_v1`
//! (the masked source a lease returns), both from wayland-protocols.
//!
//! The crate is MIT OR Apache-2.0 so the GPL fork can depend on it by git rev while nothing of
//! ours depends on the fork.

#[macro_use]
mod protocol_macro;

pub mod v1 {
    //! `quire-agent-v1`.
    wayland_protocol!(
        "../../protocols/quire-agent-v1.xml",
        [
            wayland_protocols::ext::foreign_toplevel_list::v1,
            wayland_protocols::ext::image_capture_source::v1
        ]
    );
}

#[cfg(feature = "client")]
pub use v1::client;
#[cfg(feature = "server")]
pub use v1::server;
