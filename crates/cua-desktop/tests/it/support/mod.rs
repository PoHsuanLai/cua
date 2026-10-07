#![allow(dead_code)] // each test binary uses a subset
//! A compositor in a thread, speaking just enough of stock cosmic-comp's protocols over a
//! socketpair: `ext_foreign_toplevel_list_v1`, `zcosmic_toplevel_info_v1` v3, `ext_workspace_v1`
//! and `wl_output`. Built on wayland-server; no real compositor, no live session. A test scripts
//! it with [`FakeCompositor::call`], which runs a closure on the server's thread and returns
//! once it has run and its events are flushed.

mod cosmic_server;
mod server;

pub use server::{FakeCompositor, Offer, Spec};
