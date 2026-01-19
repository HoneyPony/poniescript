use std::{ops::Deref, sync::atomic::{AtomicPtr, AtomicU64, Ordering}};

/// Marker trait for objects that have a PsObject field as their first field.
pub unsafe trait HasPsHeader {}

#[repr(C)]
pub struct PsObject {
    header: AtomicU64
}

impl PsObject {
    #[inline(always)]
    pub fn from_type_id(id: u64) -> Self {
        Self { header: id.into() }
    }
}

pub trait HasPsType {
    const TYP: u64;
}

/// A generic garbage-collected pointer.
/// 
/// This pointer does NOT perform write barriers.
/// 
/// These may only point to objects that have a PsObject field.
#[repr(transparent)]
pub struct Gp<T: HasPsHeader> {
    inner: AtomicPtr<T>
}

impl<T: HasPsHeader> Gp<T> {
    pub unsafe fn from_ptr(ptr: *mut T) -> Self {
        Self {
            inner: ptr.into()
        }
    }

    #[inline(always)]
    pub fn get_inner(&self) -> &T {
        // SAFETY: It is not valid to construct a garbage-collected pointer
        // to invalid memory.
        //
        // As such, we can always get the inner pointer.
        unsafe { &*self.inner.load(Ordering::Relaxed) }
    }

    #[inline(always)]
    pub unsafe fn get_inner_mut(&self) -> &mut T {
        // SAFETY: It is up to the caller to ensure only one thing is reading
        // or writing from this value.
        unsafe { &mut *self.inner.load(Ordering::Relaxed) }
    }
}

impl<T: HasPsHeader> Clone for Gp<T> {
    fn clone(&self) -> Self {
        Self { inner: AtomicPtr::new(self.inner.load(Ordering::Relaxed)) }
    }
}

impl<T: HasPsHeader> Deref for Gp<T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.get_inner()
    }
}

impl<T: HasPsHeader> AsRef<T> for Gp<T> {
    #[inline(always)]
    fn as_ref(&self) -> &T {
        self.get_inner()
    }
}