#![allow(dead_code)] // each test binary uses a subset
//! A compositor in a thread speaking the capture side of a stock compositor over a socketpair:
//! `ext_foreign_toplevel_list_v1`, `ext_foreign_toplevel_image_capture_source_manager_v1`,
//! `ext_image_copy_capture_manager_v1` and `wl_shm`. Built on wayland-server; every frame it
//! sends reports full damage, as stock does, and carries whatever the test last set as the
//! window's pixels. No real compositor, no live session.

mod server;

pub use server::{FakeCompositor, Window};
