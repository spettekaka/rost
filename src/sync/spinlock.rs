use core::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicUsize, Ordering},
};

pub struct Mutex<T> {
    status: AtomicUsize,
    inner: UnsafeCell<T>,
}

pub struct MutexGuard<'a, T> {
    mutex: &'a Mutex<T>,
}

#[derive(Debug)]
pub enum MutexError {}

unsafe impl<T: Send> Send for Mutex<T> {}
unsafe impl<T: Send> Sync for Mutex<T> {}

impl<T> Mutex<T> {
    pub const fn new(inner: T) -> Self {
        Self {
            status: AtomicUsize::new(0),
            inner: UnsafeCell::new(inner),
        }
    }

    pub fn lock(&self) -> Result<MutexGuard<'_, T>, MutexError> {
        loop {
            match self
                .status
                .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            {
                Ok(_) => break,     // Lock succeeded
                Err(_) => continue, // Mutex is locked, lets try again
            }
        }

        Ok(MutexGuard { mutex: self })
    }
}

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.mutex.inner.get() }
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.inner.get() }
    }
}

impl<T> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        self.mutex.status.store(0, Ordering::Release)
    }
}
