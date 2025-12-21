use std::{ffi::c_char, io::{Write, stdout}};

use crate::*;

// TODO: We will likely want to refactor the whole formatting
// system so that everything is driven by some core set of 
// functions, e.g. ps_fmt_int(output, int); this will simplify
// a lot of the code gen in the compiler.


#[unsafe(no_mangle)]
extern "C" fn ps_print_int(x: PsInt) {
    print!("{}", x);
}

#[unsafe(no_mangle)]
extern "C" fn ps_print_float(x: PsFloat) {
    print!("{}", x);
}

#[unsafe(no_mangle)]
extern "C" fn ps_print_const(what: *const c_char) {
    let mut len: usize = 0;

    unsafe {
        while *what.add(len) != 0 {
            len += 1;
        }
    }

    // TODO: This would probably be cleaner if we could write *everything*
    // in a single function call.
    let mut out = stdout().lock();
    let as_u8 = what as *const u8;
    let slice = unsafe { std::slice::from_raw_parts(as_u8, len) };

    let _ = out.write_all(slice);
}

// TODO: We really need *real* bindings for PsStr, plus a bit
// more support for strings in general. For now, this should
// be enough to get ps_print_str working.
//
// Also, this is another DST. Uh oh...
#[repr(C)]
struct TmpPsStr {
    obj: PsObject,
    // TODO: PonieScript strings should use ps_int.
    length: usize,
}

#[unsafe(no_mangle)]
extern "C" fn ps_print_str(ps_str: *const TmpPsStr) {
    let mut len: usize = 0;
    // Get the char array past the pointer.
    let contents: *const c_char = unsafe { ps_str.add(1).cast() };

    unsafe {
        while *contents.add(len) != 0 {
            len += 1;
        }
    }

    // TODO: This would probably be cleaner if we could write *everything*
    // in a single function call.
    let mut out = stdout().lock();
    let as_u8 = contents as *const u8;
    let slice = unsafe { std::slice::from_raw_parts(as_u8, len) };

    let _ = out.write_all(slice);
}

#[unsafe(no_mangle)]
extern "C" fn ps_println() {
    println!();
}

#[unsafe(no_mangle)]
extern "C" fn ps_print_bool(x: PsBool) {
    print!("{}", if x != 0 { "true" } else { "false" });
}

#[unsafe(no_mangle)]
extern "C" fn ps_print_vec2(v: Vec2) {
    print!("({}, {})", v.x, v.y);
}

#[unsafe(no_mangle)]
extern "C" fn ps_print_vec3(v: Vec3) {
    print!("({}, {}, {})", v.x, v.y, v.z);
}

#[unsafe(no_mangle)]
extern "C" fn ps_print_vec4(v: Vec4) {
    print!("({}, {}, {}, {})", v.x, v.y, v.z, v.w);
}

#[unsafe(no_mangle)]
extern "C" fn ps_print_vec2i(v: Vec2i) {
    print!("({}, {})", v.x, v.y);
}

#[unsafe(no_mangle)]
extern "C" fn ps_print_vec3i(v: Vec3i) {
    print!("({}, {}, {})", v.x, v.y, v.z);
}

#[unsafe(no_mangle)]
extern "C" fn ps_print_vec4i(v: Vec4i) {
    print!("({}, {}, {}, {})", v.x, v.y, v.z, v.w);
}