use std::{os::raw::c_void, sync::Mutex};
use macroquad::input::{KeyCode, MouseButton};
use poniescript_gc::{GcContext, HasPsHeader, HasPsType, PsBool, PsInt, PsObject, ps_bool};
use poniescript_rt::Vec2;

mod key_table;

#[unsafe(no_mangle)]
pub extern "C" fn is_key_down(_gc: &mut GcContext, key: PsInt, _closure: *mut c_void) -> PsBool {
    ps_bool(macroquad::prelude::is_key_down(key_table::convert_keycode(key)))
}

#[unsafe(no_mangle)]
pub extern "C" fn is_key_pressed(_gc: &mut GcContext, key: PsInt, _closure: *mut c_void) -> PsBool {
    ps_bool(macroquad::prelude::is_key_pressed(key_table::convert_keycode(key)))
}

#[unsafe(no_mangle)]
pub extern "C" fn is_key_released(_gc: &mut GcContext, key: PsInt, _closure: *mut c_void) -> PsBool {
    ps_bool(macroquad::prelude::is_key_released(key_table::convert_keycode(key)))
}

fn convert_mouse_code(code: PsInt) -> MouseButton {
    match code {
        0 => MouseButton::Left,
        1 => MouseButton::Middle,
        2 => MouseButton::Right,
        _ => MouseButton::Unknown
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn is_mouse_button_down(_gc: &mut GcContext, button: PsInt, _closure: *mut c_void) -> PsBool {
    ps_bool(macroquad::prelude::is_mouse_button_down(convert_mouse_code(button)))
}

#[unsafe(no_mangle)]
pub extern "C" fn is_mouse_button_pressed(_gc: &mut GcContext, button: PsInt, _closure: *mut c_void) -> PsBool {
    ps_bool(macroquad::prelude::is_mouse_button_pressed(convert_mouse_code(button)))
}

#[unsafe(no_mangle)]
pub extern "C" fn is_mouse_button_released(_gc: &mut GcContext, button: PsInt, _closure: *mut c_void) -> PsBool {
    ps_bool(macroquad::prelude::is_mouse_button_released(convert_mouse_code(button)))
}

fn vec2_t(x: (f32, f32)) -> Vec2 {
    Vec2 { x: x.0, y: x.1 }
}

fn vec2_m(x: macroquad::prelude::Vec2) -> Vec2 {
    Vec2 { x: x.x, y: x.y }
}

pub fn m_vec2(x: Vec2) -> macroquad::prelude::Vec2 {
    macroquad::prelude::Vec2 { x: x.x, y: x.y }
}

#[unsafe(no_mangle)]
pub extern "C" fn mouse_position(_gc: &mut GcContext, _closure: *mut c_void) -> Vec2 {
    vec2_t(macroquad::prelude::mouse_position())
}

#[unsafe(no_mangle)]
pub extern "C" fn mouse_position_local(_gc: &mut GcContext, _closure: *mut c_void) -> Vec2 {
    vec2_m(macroquad::prelude::mouse_position_local())
}

#[unsafe(no_mangle)]
pub extern "C" fn mouse_delta_position(_gc: &mut GcContext, _closure: *mut c_void) -> Vec2 {
    vec2_m(macroquad::prelude::mouse_delta_position())
}

#[unsafe(no_mangle)]
pub extern "C" fn pq_mouse_wheel(_gc: &mut GcContext, _closure: *mut c_void) -> Vec2 {
    vec2_t(macroquad::prelude::mouse_wheel())
}