//! Unfortunately, thanks to Rust being annoying, these have to be implemented
//! in the GC crate.
//! 
//! We honestly might want to consider merging the GC crate and RT crate into
//! just one crate.

use std::sync::atomic::AtomicI64;

use crate::HasPsType;

pub type AtomicPsInt = AtomicI64;
pub type PsFloat = f32;

pub type PsInt = i64;

// TODO:
// Is this really the best representation for PsBool? Something
// like C11's bool/_Bool would probably be better.
pub type PsBool = i8;

/// Converts a Rust bool to a PsBool. We might consider making PsBool C11 _Bool
/// so that this isn't necessary.
pub fn ps_bool(bool: bool) -> PsBool {
    if bool { 1 } else { 0 }
}

pub const PONI_TAG_FLOAT    : u64 = 0x8000000000000002;
pub const PONI_TAG_INT      : u64 = 0x8000000000000004;
pub const PONI_TAG_BOOL     : u64 = 0x8000000000000006;

pub const PONI_TAG_OPAQUE   : u64 =               0x12;

impl HasPsType for AtomicPsInt { const TYP: u64 = PONI_TAG_INT; }
impl HasPsType for PsInt       { const TYP: u64 = PONI_TAG_INT; }
impl HasPsType for PsFloat     { const TYP: u64 = PONI_TAG_FLOAT; }