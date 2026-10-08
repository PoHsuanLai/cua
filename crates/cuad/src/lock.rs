//! Locking that survives a panic elsewhere.

use std::sync::{Mutex, MutexGuard, PoisonError};

/// The guard of `mutex`. A task that panicked while holding it has poisoned it, and every later
/// call would panic too; each update here leaves its data whole, so the data is taken as it is.
pub(crate) fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_poisoned_mutex_still_gives_its_data() {
        let mutex = Mutex::new(5_u32);
        std::thread::scope(|scope| {
            let _ = scope
                .spawn(|| {
                    let _held = mutex.lock().expect("not yet poisoned");
                    panic!("a task dies holding the lock");
                })
                .join();
        });
        assert!(mutex.is_poisoned());
        assert_eq!(*locked(&mutex), 5);
    }
}
