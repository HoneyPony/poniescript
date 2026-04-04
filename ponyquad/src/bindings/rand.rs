use std::ffi::c_void;
use poniescript_gc::{GcContext, Gp, PsFloat, PsInt};

use poniescript_rt::{PsStrBuf, Vec2, Vec3, Vec4};

#[unsafe(no_mangle)]
pub extern "C" fn pq_randi_range(_ctx: &mut GcContext, left: PsInt, right: PsInt, _closure: *const c_void) -> PsInt {
    macroquad::rand::gen_range(left, right)
}

#[unsafe(no_mangle)]
pub extern "C" fn pq_randf_range(_ctx: &mut GcContext, left: PsFloat, right: PsFloat, _closure: *const c_void) -> PsFloat {
    macroquad::rand::gen_range(left, right)
}
