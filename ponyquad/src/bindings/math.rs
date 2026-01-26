use std::os::raw::c_void;
use poniescript_gc::{GcContext, PsFloat};
use poniescript_rt::{Vec2, Vec3, Vec4};

#[unsafe(no_mangle)]
pub extern "C" fn pq_norm2(_gc: &mut GcContext, mut v: Vec2, _closure: *mut c_void) -> Vec2 {
    let len = (v.x * v.x + v.y * v.y).sqrt();

    if len != 0.0 {
        v.x /= len;
        v.y /= len;
    }
    else {
        // Zero length means the vector is ~(0, 0). I guess just return that?
        v.x = 0.0;
        v.y = 0.0;
    }

    v
}

#[unsafe(no_mangle)]
pub extern "C" fn pq_norm3(_gc: &mut GcContext, mut v: Vec3, _closure: *mut c_void) -> Vec3 {
    let len = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();

    if len != 0.0 {
        v.x /= len;
        v.y /= len;
        v.z /= len;
    }
    else {
        // Zero length means the vector is ~(0, 0). I guess just return that?
        v.x = 0.0;
        v.y = 0.0;
        v.z = 0.0;
    }

    v
}

#[unsafe(no_mangle)]
pub extern "C" fn pq_norm4(_gc: &mut GcContext, mut v: Vec4, _closure: *mut c_void) -> Vec4 {
    let len = (v.x * v.x + v.y * v.y + v.z * v.z + v.w * v.w).sqrt();

    if len != 0.0 {
        v.x /= len;
        v.y /= len;
        v.z /= len;
        v.w /= len;
    }
    else {
        // Zero length means the vector is ~(0, 0). I guess just return that?
        v.x = 0.0;
        v.y = 0.0;
        v.z = 0.0;
        v.w = 0.0;
    }

    v
}

#[unsafe(no_mangle)]
pub extern "C" fn pq_len2(_gc: &mut GcContext, v: Vec2, _closure: *mut c_void) -> PsFloat {
    (v.x * v.x + v.y * v.y).sqrt()
}

#[unsafe(no_mangle)]
pub extern "C" fn pq_len3(_gc: &mut GcContext, v: Vec3, _closure: *mut c_void) -> PsFloat {
    (v.x * v.x + v.y * v.y + v.z * v.z).sqrt()
}

#[unsafe(no_mangle)]
pub extern "C" fn pq_len4(_gc: &mut GcContext, v: Vec4, _closure: *mut c_void) -> PsFloat {
    (v.x * v.x + v.y * v.y + v.z * v.z + v.w * v.w).sqrt()
}

#[unsafe(no_mangle)]
pub extern "C" fn pq_atan2(_gc: &mut GcContext, v: Vec2, _closure: *mut c_void) -> PsFloat {
    v.y.atan2(v.x)
}