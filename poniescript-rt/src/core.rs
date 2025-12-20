use std::sync::atomic::{AtomicI64, AtomicU64};

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

pub type PsInt = AtomicI64;
pub type PsFloat = f32;

pub trait HasPsType {
    const TYP: u64;
}