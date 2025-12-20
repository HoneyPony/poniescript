
use crate::*;

use std::sync::atomic::{AtomicPtr, AtomicU64};

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