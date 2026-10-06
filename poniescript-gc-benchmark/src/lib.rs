use std::{os::raw::c_void, time::{Duration, Instant}};

use poniescript_gc::GcContext;

#[unsafe(no_mangle)]
pub fn main() {
    env_logger::init();

    let mut gc_handle = poniescript_gc::gc_spawn();
    let mut ctx = gc_handle.create_context_for_existing();

    let ctx = ctx.as_mut();

    unsafe extern "C" {
        fn poni_init_strings(_ctx: &mut GcContext);
        fn poni_init_globals(_ctx: &mut GcContext);

        fn update(_ctx: &mut GcContext, closure: *mut c_void);
    }

    unsafe {
        poni_init_strings(ctx);
        poni_init_globals(ctx);
    }

    eprintln!("initialized globals!");
    let mut worst_time = Duration::from_secs(0);

    loop {
        unsafe { update(ctx, std::ptr::null_mut()); }

        let start = Instant::now();
        ctx.poll_slow();
        let duration = start.elapsed();
        if duration > worst_time {
            worst_time = duration;
            eprintln!("new worst time: {:?}", worst_time);
        }
        //eprintln!("gc time: {:?}", duration);
    }
}