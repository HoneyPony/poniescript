use core::slice;
use std::{alloc::Layout, borrow::Cow, sync::atomic::Ordering};

use poniescript_gc::{AtomicPsInt, GcContext, Gp, HasPsHeader, HasPsType, PsInt, PsObject};

use crate::{PONI_TAG_STR, PONI_TAG_STRBUF};

unsafe impl HasPsHeader for PsStr {}
impl HasPsType for PsStr {
    const TYP: u64 = PONI_TAG_STR;
}
unsafe impl HasPsHeader for PsStrBuf {}
impl HasPsType for PsStrBuf {
    const TYP: u64 = PONI_TAG_STRBUF;
}

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

    pub fn get_data(&self) -> &[u8] {
        unsafe {
            let data_ptr = self.get_data_ptr();
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

#[repr(C)]
pub struct PsStrBuf {
    obj: PsObject,

    buffer: Gp<PsStr>,
    length: AtomicPsInt,
}

impl PsStrBuf {
    pub fn from_string(gc: &mut GcContext, str: &str) -> Gp<PsStrBuf> {
        let bytes = str.as_bytes();
        let total_len = std::mem::size_of::<PsStr>() + bytes.len();
        let alloc = gc.alloc_raw_bytes(total_len);

        let ps_str = unsafe {
            let header: *mut PsStr = alloc as _;
            let contents: *mut u8 = alloc.byte_add(std::mem::size_of::<PsStr>()) as _;

            *header = PsStr { obj: PsObject::from_type_id(PONI_TAG_STR), length: (bytes.len() as i64).into() };
            contents.copy_from(bytes.as_ptr(), bytes.len());

            Gp::from_ptr(alloc as *mut PsStr)
        };

        let str_buf = gc.alloc(PsStrBuf {
            obj: PsObject::from_type_id(PONI_TAG_STRBUF),
            buffer: ps_str,
            length: (bytes.len() as i64).into()
        });

        str_buf
    }

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