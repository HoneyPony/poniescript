
use crate::*;

use std::{marker::PhantomData, path::Iter, sync::atomic::{AtomicPtr, AtomicU64, Ordering}};

#[repr(C)]
struct PsArrayHeader {
    obj: PsObject,
    typ: AtomicU64,
    length: AtomicPsInt,
}

#[repr(C)]
struct PsDynArrayHeader {
    obj: PsObject,
    typ: AtomicU64,
    length: AtomicPsInt,
    buffer: AtomicPtr<PsArrayHeader>,
}

pub struct PsArray<T: Sized + HasPsType> {
    header: PsArrayHeader,
    data: PhantomData<[T]>,
}

unsafe impl<T: Sized + HasPsType> HasPsHeader for PsArray<T> {}

impl<T: Sized + HasPsType> PsArray<T> {
    pub unsafe fn from_len_usize(ctx: &mut GcContext, len: usize) -> Option<Gp<Self>> {
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

        let as_array = allocated as *mut PsArray<T>;

        Some(Gp::from_ptr(as_array))
    }

    #[inline(always)]
    unsafe fn get_data_ptr_from_header(header: &PsArrayHeader) -> *mut T {
        // Add is in terms of size_of
        let data = header as *const PsArrayHeader;
        let data = data.add(1);
        data as *mut T
    }

    #[inline(always)]
    unsafe fn get_data_ptr(&self) -> *mut T {
        Self::get_data_ptr_from_header(&self.header)
    }

    #[inline(always)]
    pub fn len(&self) -> PsInt {
        self.header.length.load(Ordering::Relaxed)
    }

    pub fn get(&self, index: PsInt) -> &T {
        unsafe {
             // Boundary check
            if index < 0 || index >= self.header.length.load(Ordering::Relaxed) {
                // TODO: PonieScript panic (?)
                // Maybe not.
                panic!("index out of bounds");
            }

            let data = self.get_data_ptr();
            // TODO: This index -> isize cast safely..?
            &*data.offset(index as isize)
        }
    }
}

impl<T: Sized + HasPsType + Clone> PsArray<T> {
    /// Constructs a new PsArray from a slice of Cloneable contents.
    /// 
    /// Each object in the slice will be cloned into the array.
    pub fn from_slice(ctx: &mut GcContext, slice: &[T]) -> Option<Gp<Self>> {
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
        unsafe {
            // Boundary check
            if index < 0 || index >= self.header.length.load(Ordering::Relaxed) {
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

impl<T: Sized + HasPsType> PsArray<T> {
    /// Constructs a new PsArray from a slice of Cloneable contents.
    /// 
    /// Each object in the slice will be cloned into the array.
    pub fn from_slice_into<U>(ctx: &mut GcContext, slice: &[U]) -> Option<Gp<Self>>
    where 
        U: Into<T> + Clone
    {
        unsafe {
            let me = Self::from_len_usize(ctx, slice.len())?;
            let data = me.get_data_ptr();

            for (idx, obj) in slice.iter().enumerate() {
                // TODO: Is there any way to reduce the number of try_into()'s here?
                // Probably...
                std::ptr::write(data.offset(idx.try_into().ok()?), obj.clone().into());
            }

            Some(me)
        }
    }
}

#[cfg(test)]
mod test {
    use poniescript_gc::gc_spawn;
    use super::*;

    // Shouldn't panic.
    #[test]
    fn allocate() {
        let mut handle = gc_spawn();
        let mut ctx = handle.create_context_for_existing();

        let slice: &[i64] = &[1, 2, 3, 4];

        let _arr: Gp<PsArray<AtomicPsInt>> = PsArray::from_slice_into(&mut ctx, slice).unwrap();
    }
    
    #[test]
    fn uh_oh() {
        // This shouldn't compile. PsArray<> should be !Sized. :(
        let _arr: PsArray<AtomicPsInt> = PsArray {
            header: PsArrayHeader {
                obj: PsObject::from_type_id(0),
                typ: 0.into(),
                length: 0.into()
            },
            data: PhantomData
        };
    }

    #[test]
    fn set_and_get() {
        let mut handle = gc_spawn();
        let mut ctx = handle.create_context_for_existing();

        let slice: &[i64] = &[1, 2, 3, 4];

        let arr: Gp<PsArray<AtomicPsInt>> = PsArray::from_slice_into(&mut ctx, slice).unwrap();
        for i in 0..arr.len() {
            let a = arr.get(i).load(Ordering::Relaxed);
            let b = slice[i as usize];
            assert_eq!(a, b);
        }
    }

    #[test]
    #[should_panic]
    fn too_big_idx() {
        let mut handle = gc_spawn();
        let mut ctx = handle.create_context_for_existing();

        let slice: &[i64] = &[1, 2, 3, 4];

        let arr: Gp<PsArray<AtomicPsInt>> = PsArray::from_slice_into(&mut ctx, slice).unwrap();
        let _x = arr.get(5);
    }

    #[test]
    #[should_panic]
    fn too_small_idx() {
        let mut handle = gc_spawn();
        let mut ctx = handle.create_context_for_existing();

        let slice: &[i64] = &[1, 2, 3, 4];

        let arr: Gp<PsArray<AtomicPsInt>> = PsArray::from_slice_into(&mut ctx, slice).unwrap();
        let _x = arr.get(-1);
    }
}