//! **What the browser's build of the decision runs on instead of `std`**:
//! one thread, a heap that only grows, and a panic that stops.
//!
//! `std`'s allocator, its panic machinery and the formatting those reach were
//! most of the module every interactive page downloads before it can attach a
//! handler. The decision needs none of them: it runs on one thread, allocates
//! a few short-lived values for each answer, and has nobody to tell a panic to.

use alloc::alloc::{GlobalAlloc, Layout};
use core::cell::{Cell, UnsafeCell};
use core::convert::Infallible;
use core::ops::{Deref, DerefMut};

const PAGE: usize = 65_536;

/// **A heap that only grows**, in pages it asks the module's memory for. Each
/// decision's values are given back together, by [`scope`].
struct Heap {
    next: Cell<usize>,
    end: Cell<usize>,
}

// `wasm32-unknown-unknown` has one thread.
unsafe impl Sync for Heap {}

#[global_allocator]
static HEAP: Heap = Heap {
    next: Cell::new(0),
    end: Cell::new(0),
};

unsafe impl GlobalAlloc for Heap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // The top of the memory the module started with, the first time.
        if self.end.get() == 0 {
            let top = core::arch::wasm32::memory_size(0) * PAGE;
            self.next.set(top);
            self.end.set(top);
        }
        let start = (self.next.get() + layout.align() - 1) & !(layout.align() - 1);
        let end = start + layout.size();
        if end > self.end.get() {
            let pages = (end - self.end.get()).div_ceil(PAGE);
            if core::arch::wasm32::memory_grow(0, pages) == usize::MAX {
                return core::ptr::null_mut();
            }
            self.end.set(self.end.get() + pages * PAGE);
        }
        self.next.set(end);
        start as *mut u8
    }

    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}

/// **Everything allocated while this lives is given back when it ends.** What
/// a decision allocates is its own: nothing it allocates outlives its answer,
/// so a page that decides for many handlers uses no more than it needs for
/// one.
pub(crate) struct Scope(usize);

pub(crate) fn scope() -> Scope {
    Scope(HEAP.next.get())
}

impl Drop for Scope {
    fn drop(&mut self) {
        // Zero is a heap not begun when this scope was: what was allocated
        // since began it, and goes on being usable.
        if self.0 != 0 {
            HEAP.next.set(self.0);
        }
    }
}

/// A panic has nobody to tell: it stops the module, and the page's refusal
/// path treats a decision that traps as a decision not made.
#[panic_handler]
fn stops(_: &core::panic::PanicInfo<'_>) -> ! {
    core::arch::wasm32::unreachable()
}

/// `std`'s lock, as this module's one thread uses it.
pub(crate) struct Mutex<T>(UnsafeCell<T>);

unsafe impl<T> Sync for Mutex<T> {}

pub(crate) struct Guard<'a, T>(&'a mut T);

impl<T> Mutex<T> {
    pub(crate) const fn new(value: T) -> Self {
        Mutex(UnsafeCell::new(value))
    }

    pub(crate) fn lock(&self) -> Result<Guard<'_, T>, Infallible> {
        // One thread, and no guard outlives its statement.
        Ok(Guard(unsafe { &mut *self.0.get() }))
    }
}

impl<T> Deref for Guard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.0
    }
}

impl<T> DerefMut for Guard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.0
    }
}
