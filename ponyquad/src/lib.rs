mod bindings;
mod hot;

use macroquad::prelude::*;
use macroquad::logging::*;

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
pub extern "C" fn main() {
    info!("ponyquad: we got this far!");
    // set_panic_handler(async |a, b| {
    //     info!("ponyquad: panic: {} {}", a, b);
    // });
    info!("ponyquad: another message! :)");
    
    macroquad::Window::new("Game", async { macroquad_main().await });
    info!("ponyquad: idk what this means!");
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

async fn macroquad_main() {
    info!("ponyquad: got to macroquad_main!");
    let mut gc_handle = gc_spawn();
    let mut ctx = gc_handle.create_context_for_existing();

    let ctx = ctx.as_mut();

    let mut hot = hot::HotReload::new("./.build/hot/script-init.so", ctx).unwrap();

    loop {
        hot::call_update(ctx, &mut hot);
        hot.poll(ctx);

        bindings::texture::process_queue().await;

        //clear_background(BEIGE);

        next_frame().await
    }
}
