use std::{cell::RefCell, ffi::c_void};

use poniescript_gc::GcContext;
use poniescript_rt::Vec2;

struct UnsafeCanvas {
    pub inner: RefCell<Option<macroquad_canvas::Canvas2D>>,
}

/// SAFETY: ponyquad is single-threaded.
unsafe impl Sync for UnsafeCanvas {}

static CANVAS: UnsafeCanvas = UnsafeCanvas { inner: RefCell::new(None) };

pub fn frame_begin() {
    let canvas = CANVAS.inner.borrow();
    if let Some(inner) = canvas.as_ref() {
        macroquad::prelude::set_camera(&inner.camera);
    }
}

pub fn frame_end() {
    let canvas = CANVAS.inner.borrow();
    if let Some(inner) = canvas.as_ref() {
        macroquad::prelude::set_default_camera();
        inner.draw();
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn set_canvas(_gc: &mut GcContext, size: Vec2, _closure: *mut c_void) {
    let mut canvas = CANVAS.inner.borrow_mut();
    *canvas = Some(macroquad_canvas::Canvas2D::new(size.x, size.y));
}

#[unsafe(no_mangle)]
pub extern "C" fn clear_canvas(_gc: &mut GcContext, _closure: *mut c_void) {
    let mut canvas = CANVAS.inner.borrow_mut();
    *canvas = None;
}