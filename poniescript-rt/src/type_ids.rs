//! Core type IDs for PonieScript.
//! 
//! The compiler knows not to ever generate anything clashing with these.

pub const PONI_TAG_FLOAT    : u64 = 0x8000000000000002;
pub const PONI_TAG_INT      : u64 = 0x8000000000000004;
pub const PONI_TAG_BOOL     : u64 = 0x8000000000000006;
pub const PONI_TAG_STRCONST : u64 = 8;
pub const PONI_TAG_STR      : u64 = 10;
pub const PONI_TAG_STRBUF   : u64 = 12;
pub const PONI_TAG_ARRAY    : u64 = 14;
pub const PONI_TAG_DYNARRAY : u64 = 16;