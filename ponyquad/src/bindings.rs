pub mod texture;
pub mod input;

use std::ffi::c_void;
use poniescript_gc::{GcContext, Gp, PsFloat, PsInt};

use poniescript_rt::{PsStrBuf, Vec2, Vec3, Vec4};

#[unsafe(no_mangle)]
pub extern "C" fn clear_background(_ctx: &mut GcContext, color: Vec4, _closure: *const c_void) {
    unsafe {
        macroquad::prelude::clear_background(
            std::mem::transmute(color)
        )
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn draw_line(_ctx: &mut GcContext, from: Vec2, to: Vec2, thickness: f32, color: Vec4, _closure: *const c_void) {
    unsafe {
        macroquad::prelude::draw_line(
            from.x, from.y,
            to.x, to.y,
            thickness,
            std::mem::transmute(color)
        )
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn draw_rectangle(_ctx: &mut GcContext, at: Vec2, size: Vec2, color: Vec4, _closure: *const c_void) {
    unsafe {
        macroquad::prelude::draw_rectangle(
            at.x, at.y,
            size.x, size.y,
            std::mem::transmute(color)
        )
    }
}

// We need a PsStrBuf type...
#[unsafe(no_mangle)]
pub extern "C" fn draw_text(_ctx: &mut GcContext, text: Gp<PsStrBuf>, at: Vec2, font_size: f32, color: Vec4, _closure: *const c_void) -> Vec3 {
    unsafe {
        let text = text.get_inner();
        let text = text.get_string();

        let dims = macroquad::prelude::draw_text(
            &text,
            at.x, at.y,
            font_size,
            std::mem::transmute(color)
        );

        Vec3 { x: dims.width, y: dims.height, z: dims.offset_y }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn screen_width(_ctx: &mut GcContext, _closure: *const c_void) -> PsFloat {
    macroquad::prelude::screen_width()
}

#[unsafe(no_mangle)]
pub extern "C" fn screen_height(_ctx: &mut GcContext, _closure: *const c_void) -> PsFloat {
    macroquad::prelude::screen_height()
}

#[unsafe(no_mangle)]
pub extern "C" fn get_frame_time(_ctx: &mut GcContext, _closure: *const c_void) -> PsFloat {
    macroquad::prelude::get_frame_time()
}

#[unsafe(no_mangle)]
pub extern "C" fn get_time(_ctx: &mut GcContext, _closure: *const c_void) -> PsFloat {
    // This is... not great, to say the least. We might want to implement doubles...?
    macroquad::prelude::get_time() as PsFloat
}

// Return PsInt because, even though float would make more sense, Int is more
// honest that the API can't return a float.
#[unsafe(no_mangle)]
pub extern "C" fn get_fps(_ctx: &mut GcContext, _closure: *const c_void) -> PsInt {
    macroquad::prelude::get_fps() as PsInt
}