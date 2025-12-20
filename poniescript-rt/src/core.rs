use std::sync::atomic::{AtomicI64, AtomicU64};

#[repr(C)]
pub struct PsObject {
    header: AtomicU64
}

pub type PsInt = AtomicI64;
pub type PsFloat = f32;