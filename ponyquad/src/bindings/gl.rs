use std::ffi::c_void;

use macroquad::prelude::*;

use poniescript_gc::{GcContext, Gp, PsFloat, PsInt};
use poniescript_rt::{PsStrBuf, Vec2, Vec3, Vec4};

#[unsafe(no_mangle)]
pub extern "C" fn gl_push_rotation(_gc: &mut GcContext, rotation: PsFloat, _closure: *mut c_void) {
    let gl = unsafe { get_internal_gl().quad_gl };
    gl.push_model_matrix(glam::Mat4::from_rotation_z(rotation));
}

#[unsafe(no_mangle)]
pub extern "C" fn gl_push_scale(_gc: &mut GcContext, scale: Vec2, _closure: *mut c_void) {
    let gl = unsafe { get_internal_gl().quad_gl };
    gl.push_model_matrix(glam::Mat4::from_scale(glam::Vec3 {
        x: scale.x,
        y: scale.y,
        z: 1.0
    }));
}

#[unsafe(no_mangle)]
pub extern "C" fn gl_push_translation(_gc: &mut GcContext, translation: Vec2, _closure: *mut c_void) {
    let gl = unsafe { get_internal_gl().quad_gl };
    gl.push_model_matrix(glam::Mat4::from_translation(glam::Vec3 {
        x: translation.x,
        y: translation.y,
        z: 1.0
    }));
}

#[unsafe(no_mangle)]
pub extern "C" fn gl_pop(_gc: &mut GcContext, translation: Vec2, _closure: *mut c_void) {
    let gl = unsafe { get_internal_gl().quad_gl };
    gl.pop_model_matrix();
}