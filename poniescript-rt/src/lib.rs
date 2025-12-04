use std::os::raw::c_void;

use poniescript_gc::{GcContext, gc_spawn};

extern "C" {
    fn poni_init_strings(ctx: &mut GcContext);
    fn poni_init_globals(ctx: &mut GcContext);
    fn poni_init(ctx: &mut GcContext);
}

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let mut gc_handle = gc_spawn();
    let mut ctx = gc_handle.create_context_for_existing();

    let ctx = ctx.as_mut();

    // Must initialize strings before globals
    unsafe {
        poni_init_strings(ctx);
        poni_init_globals(ctx);
        poni_init(ctx);
    }

    return 0;
}