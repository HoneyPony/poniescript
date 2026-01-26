use std::os::raw::c_void;
use macroquad::camera::Camera2D;
use poniescript_gc::{GcContext, Gp, GpMaybe, HasPsHeader, PsFloat, PsObject};
use poniescript_rt::{Vec2, Vec3, Vec4};

use crate::bindings::input::{m_vec2};

#[repr(C)]
pub struct PsCamera {
    object: PsObject,

    pub rotation: PsFloat,
    pub zoom: Vec2,
    pub target: Vec2,
    pub offset: Vec2,

    // It would be nice to make this an optional value type, but that's OK.
    pub viewport: GpMaybe<PsCameraViewport>
}

#[repr(C)]
pub struct PsCameraViewport {
    object: PsObject,

    offset: Vec2,
    size: Vec2,
}

// TODO: Add type ids for all these classes...
unsafe impl HasPsHeader for PsCamera {}
unsafe impl HasPsHeader for PsCameraViewport {}

#[unsafe(no_mangle)]
pub extern "C" fn set_camera(_gc: &mut GcContext, camera: Gp<PsCamera>, _closure: *mut c_void) {
    let inner = camera.get_inner();
    
    let camera = Camera2D {
        rotation: inner.rotation,
        zoom: m_vec2(inner.zoom),
        target: m_vec2(inner.target),
        offset: m_vec2(inner.offset),
        render_target: None,
        viewport: inner.viewport.get_inner().map(|i| {
            (i.offset.x as i32, i.offset.y as i32, i.size.x as i32, i.size.y as i32)
        }),
    };

    macroquad::prelude::set_camera(&camera);
}

#[unsafe(no_mangle)]
pub extern "C" fn set_default_camera(_gc: &mut GcContext, _closure: *mut c_void) {
    macroquad::prelude::set_default_camera();
}