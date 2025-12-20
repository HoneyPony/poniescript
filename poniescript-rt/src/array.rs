
use crate::*;

use std::{marker::PhantomData, path::Iter, sync::atomic::{AtomicPtr, AtomicU64, Ordering}};

#[repr(C)]
struct PsArrayHeader {
    obj: PsObject,
    typ: AtomicU64,
    length: PsInt,
}

#[repr(C)]
struct PsDynArrayHeader {
    obj: PsObject,
    typ: AtomicU64,
    length: PsInt,
    buffer: AtomicPtr<PsArrayHeader>,
}

pub struct PsArray<T: Sized + HasPsType> {
    header: AtomicPtr<PsArrayHeader>,
    data: PhantomData<T>,
}

impl<T: Sized + HasPsType> PsArray<T> {
    pub unsafe fn from_len_usize(ctx: &mut GcContext, len: usize) -> Option<Self> {
        // Number of bytes needed for the allocation
        let bytes = len.checked_mul(size_of::<T>())?.checked_add(size_of::<PsArrayHeader>())?;

        // Lnegth value as a PsInt
        let len: i64 = len.try_into().ok()?;

        let header = PsArrayHeader {
            obj: PsObject::from_type_id(PONI_TAG_ARRAY),
            typ: T::TYP.into(),
            length: len.into(),
        };

        let allocated = ctx.alloc(bytes);
        let as_header = allocated as *mut PsArrayHeader;
        *as_header = header;

        Some(Self {
            header: as_header.into(),
            data: PhantomData,
        })
    }

    #[inline(always)]
    unsafe fn get_data_ptr_from_header(header: *mut PsArrayHeader) -> *mut T {
        // Add is in terms of size_of
        let data = header.add(1);
        data.cast()
    }

    #[inline(always)]
    unsafe fn get_data_ptr(&self) -> *mut T {
        Self::get_data_ptr_from_header(self.header.load(Ordering::Relaxed))
    }
}

impl<T: Sized + HasPsType + Clone> PsArray<T> {
    /// Constructs a new PsArray from a slice of Cloneable contents.
    /// 
    /// Each object in the slice will be cloned into the array.
    pub fn from_slice(ctx: &mut GcContext, slice: &[T]) -> Option<Self> {
        unsafe {
            let me = Self::from_len_usize(ctx, slice.len())?;
            let data = me.get_data_ptr();

            for (idx, obj) in slice.iter().enumerate() {
                // TODO: Is there any way to reduce the number of try_into()'s here?
                // Probably...
                std::ptr::write(data.offset(idx.try_into().ok()?), obj.clone());
            }

            Some(me)
        }
    }

    pub fn get_cloned(&self, index: i64) -> T {
        let header = self.header.load(Ordering::Relaxed);

        unsafe {
            // Boundary check
            if index < 0 || index >= (*header).length.load(Ordering::Relaxed) {
                // TODO: PonieScript panic (?)
                // Maybe not.
                panic!("index out of bounds");
            }

            let data = self.get_data_ptr();
            // TODO: This index -> isize cast safely..?

            // TODO: We probably don't want to use ptr::read here? Seems incorrect.
            (*data.offset(index as isize)).clone()
        }
    }
}