use core::slice;
use std::{borrow::Cow, sync::atomic::Ordering};

use poniescript_gc::{AtomicPsInt, Gp, HasPsHeader, PsInt, PsObject};

unsafe impl HasPsHeader for PsStr {}
unsafe impl HasPsHeader for PsStrBuf {}

#[repr(C)]
pub struct PsStr {
    obj: PsObject,

    length: AtomicPsInt,
    // The contents are trailing. We should be a DST even though we aren't.
}

impl PsStr {
    #[inline(always)]
    unsafe fn get_data_ptr(&self) -> *mut u8 {
        // Add is in terms of size_of
        let data = self as *const PsStr;
        let data = data.add(1);
        data as *mut u8
    }
}

#[repr(C)]
pub struct PsStrBuf {
    obj: PsObject,

    buffer: Gp<PsStr>,
    length: AtomicPsInt,
}

impl PsStrBuf {
    pub fn get_data(&self) -> &[u8] {
        unsafe {
            let data_ptr = self.buffer.get_data_ptr();
            let len = self.length.load(Ordering::Relaxed);
            let len = len.try_into().expect("length");
            slice::from_raw_parts(data_ptr, len)
        }
    }

    pub fn get_string(&self) -> Cow<'_, str> {
        let data = self.get_data();
        let utf8 = String::from_utf8_lossy(data);

        utf8
    }
}