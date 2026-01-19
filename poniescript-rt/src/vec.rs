use poniescript_gc::{PsFloat, PsInt};

// TODO:
// Consider using some common Rust vec type as our vec type.
// Would probably be helpful.
//
// Also consider generating these with macros.

#[repr(C)]
pub struct Vec2 {
    pub x: PsFloat,
    pub y: PsFloat,
}

#[repr(C)]
pub struct Vec3 {
    pub x: PsFloat,
    pub y: PsFloat,
    pub z: PsFloat,
}

#[repr(C)]
pub struct Vec4 {
    pub x: PsFloat,
    pub y: PsFloat,
    pub z: PsFloat,
    pub w: PsFloat,
}

#[repr(C)]
pub struct Vec2i {
    pub x: PsInt,
    pub y: PsInt,
}

#[repr(C)]
pub struct Vec3i {
    pub x: PsInt,
    pub y: PsInt,
    pub z: PsInt,
}

#[repr(C)]
pub struct Vec4i {
    pub x: PsInt,
    pub y: PsInt,
    pub z: PsInt,
    pub w: PsInt,
}