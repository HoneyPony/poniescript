mod bindings;
mod hot;

use macroquad::prelude::*;

use poniescript_gc::gc_spawn;

// unsafe extern "C" {
//     // The PonieScript update function that we want to call into.
//     fn f_update(gc: &mut GcContext, closure: *const c_void);
// }

// We need an unsafe(no_mangle) main so that we can link ourselves as the main
// method against the PonieScript script.
//
// This also means we can't directly use Macroquad's #[main] macro; instead,
// manually do what it does (which is just calling a constructor on
// macroquad::Window).
#[unsafe(no_mangle)]
#[cfg(not(feature = "hotreload"))]
pub extern "C" fn main() {
    macroquad::Window::new("Game", async { macroquad_main().await });
}

#[unsafe(no_mangle)]
#[cfg(not(feature = "hotreload"))]
#[cfg(target_arch = "wasm32")]
pub extern "C" fn _start() {
    main();
}

// #[cfg(not(feature = "hotreload"))]
// #[macroquad::main("Game")]
// async fn main() {
//     macroquad_main().await
// }

/// Should be called by the hotreload host.
#[cfg(feature = "hotreload")]
pub fn ponyquad_main() {
    macroquad::Window::new("Game", macroquad_main());
}

#[cfg(target_arch = "wasm32")]
struct LogConnector {}
#[cfg(target_arch = "wasm32")]
impl Log for LogConnector {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        let message = record.args().to_string();
        miniquad::log::__private_api_log_lit(&message, miniquad::log::Level::Info, &(
            record.target(), record.module_path_static().unwrap_or("unknown"),
            record.file_static().unwrap_or("unknown"), record.line().unwrap_or(0)
        ));
    }

    fn flush(&self) {
        
    }
}

async fn macroquad_main() {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = macroquad::logging::set_logger(&LogConnector{});
        set_max_level(LevelFilter::Trace);
    }

    #[cfg(all(not(target_arch = "wasm32"), debug_assertions))]
    env_logger::init();

    let mut gc_handle = gc_spawn();
    let mut ctx = gc_handle.create_context_for_existing();

    let ctx = ctx.as_mut();

    let mut hot = hot::HotReload::new("./.build/hot/script-init.so", ctx).unwrap();

    loop {
        bindings::canvas::frame_begin();

        hot::call_update(ctx, &mut hot);
        hot.poll(ctx);

        bindings::canvas::frame_end();

        bindings::texture::process_queue().await;

        // Poll the gc. We are in theory going to compile the script code to NOT
        // safepoint at all, so we have to do it here.
        ctx.poll_slow();

        //clear_background(BEIGE);

        next_frame().await
    }
}
