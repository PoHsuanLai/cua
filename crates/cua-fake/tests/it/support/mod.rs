#![allow(dead_code, unused_imports)] // each test binary uses a subset of the shared builders
//! The scenes of `cua_fake::scene`, and an executor for futures that never wait.

pub use cua_fake::scene::*;

/// A minimal executor for futures that never wait.
pub fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    let mut future = std::pin::pin!(future);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
    }
}
