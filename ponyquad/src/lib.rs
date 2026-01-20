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
pub fn main() {
    macroquad::Window::new("Game", macroquad_main());
}

/// Should be called by the hotreload host.
#[cfg(feature = "hotreload")]
pub fn ponyquad_main() {
    macroquad::Window::new("Game", macroquad_main());
}

async fn macroquad_main() {
    let mut gc_handle = gc_spawn();
    let mut ctx = gc_handle.create_context_for_existing();

    let ctx = ctx.as_mut();

    let mut hot = hot::HotReload::new("./.build/hot/script-init.so", ctx).unwrap();

    loop {
        hot::call_update(ctx, &mut hot);
        hot.poll(ctx);

        bindings::texture::process_queue().await;

        next_frame().await
    }
}
