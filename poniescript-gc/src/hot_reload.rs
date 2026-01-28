//! Support for hot reloading, and for not hot reloading.
//! 
//! Wraps the interface to actually call the various poni_gc functions, which
//! differs between hot-reload and not.

use std::{marker::PhantomData, sync::atomic::{AtomicUsize, Ordering}};

use crate::Gc;

#[cfg(not(feature = "hotreload"))]
extern "C" {
    fn poni_gc_visit_object(gc: &mut Gc, ptr: *mut u64);
    fn poni_gc_visit_roots(gc: &mut Gc);
    fn poni_gc_get_allocation_size(ptr: *mut u64) -> usize;
}

#[cfg(feature = "hotreload")]
struct PsuedoAtomicFnPtr<T> {
    inner: AtomicUsize,
    phantom: PhantomData<T>
}

#[cfg(feature = "hotreload")]
impl<T> PsuedoAtomicFnPtr<T> {
    fn store(&self, ptr: T) {
        self.inner.store(unsafe { std::mem::transmute_copy(&ptr) }, Ordering::Relaxed)
    }

    fn load(&self) -> T {
        let val = self.inner.load(Ordering::Relaxed);
        if val == 0 { panic!("null pointer"); }
        unsafe { std::mem::transmute_copy(&val) }
    }

    const fn null() -> Self {
        PsuedoAtomicFnPtr { inner: AtomicUsize::new(0), phantom: PhantomData }
    }
}

#[cfg(feature = "hotreload")]
static GC_VISIT_OBJECT: PsuedoAtomicFnPtr<fn(&mut Gc, *mut u64)> = PsuedoAtomicFnPtr::null();
#[cfg(feature = "hotreload")]
static GC_VISIT_ROOTS: PsuedoAtomicFnPtr<fn(&mut Gc)> = PsuedoAtomicFnPtr::null();
#[cfg(feature = "hotreload")]
static GC_GET_ALLOCATION_SIZE: PsuedoAtomicFnPtr<fn(*mut u64) -> usize> = PsuedoAtomicFnPtr::null();

/// Provide dynamically loaded GC runtime functions.
#[cfg(feature = "hotreload")]
pub fn load_gc_functions(visit_obj: fn(&mut Gc, *mut u64), visit_roots: fn(&mut Gc), allocation_size: fn(*mut u64) -> usize) {
    GC_VISIT_OBJECT.store(visit_obj);
    GC_VISIT_ROOTS.store(visit_roots);
    GC_GET_ALLOCATION_SIZE.store(allocation_size);
}

#[inline(always)]
pub unsafe fn gc_visit_object(gc: &mut Gc, ptr: *mut u64) {
    // We assume objects that are in the mark queue have already been
    // marked, and so will unconditionally be visited.
    #[cfg(feature = "hotreload")]
    {
        let gc_visit_object = GC_VISIT_OBJECT.load();
        // This is a little slow, but oh well. One thing we could consider
        // in terms of linking against a shared library is that this
        // is not actually necessary unless we are *reloading* the
        // symbols.
        gc_visit_object(gc, ptr);
    }
    #[cfg(not(feature = "hotreload"))]
    unsafe {
        poni_gc_visit_object(gc, ptr);
    }
}

#[inline(always)]
pub unsafe fn gc_get_allocation_size(ptr: *mut u64) -> usize {
    #[cfg(feature = "hotreload")]
    {
        let gc_get_allocation_size = GC_GET_ALLOCATION_SIZE.load();
        // This is a little slow, but oh well. One thing we could consider
        // in terms of linking against a shared library is that this
        // is not actually necessary unless we are *reloading* the
        // symbols.
        gc_get_allocation_size(ptr)
    }
    #[cfg(not(feature = "hotreload"))]
    unsafe {
        poni_gc_get_allocation_size(ptr)
    }
}

#[inline(always)]
pub unsafe fn gc_visit_roots(gc: &mut Gc) {
    #[cfg(feature = "hotreload")]
    {
        let gc_visit_roots = GC_VISIT_ROOTS.load();
        // This is a little slow, but oh well. One thing we could consider
        // in terms of linking against a shared library is that this
        // is not actually necessary unless we are *reloading* the
        // symbols.
        gc_visit_roots(gc);
    }
    #[cfg(not(feature = "hotreload"))]
    unsafe {
        poni_gc_visit_roots(gc);
    }
}