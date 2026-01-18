mod hot;

use macroquad::prelude::*;
use notify::{Event, RecursiveMode, Watcher};

use std::{ffi::{OsStr, c_void}, fs, path::Path, process::Command, ptr, sync::mpsc};
use poniescript_gc::{GcContext, gc_spawn};

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
// #[unsafe(no_mangle)]
// pub fn main() {
//     macroquad::Window::new("Game", macroquad_main());
// }



#[macroquad::main("Game")]
async fn main() {
    let mut gc_handle = gc_spawn();
    let mut ctx = gc_handle.create_context_for_existing();

    let ctx = ctx.as_mut();

    let mut hot = hot::HotReload::new("./game-script.so").unwrap();

    loop {
        clear_background(RED);

        draw_line(40.0, 40.0, 100.0, 200.0, 15.0, BLUE);
        draw_rectangle(screen_width() / 2.0 - 60.0, 100.0, 120.0, 60.0, GREEN);

        draw_text("Hello, Macroquad!", 20.0, 20.0, 30.0, DARKGRAY);

        hot::call_update(ctx, &mut hot);
        hot.poll();

        

        

        next_frame().await
    }
}

mod bindings {
    use std::ffi::c_void;
    use poniescript_gc::GcContext;

    use poniescript_rt::{Vec2, Vec4};

    #[unsafe(no_mangle)]
    pub extern "C" fn draw_line(_ctx: &mut GcContext, from: Vec2, to: Vec2, thickness: f32, color: Vec4, _closure: *const c_void) {
        unsafe {
            macroquad::prelude::draw_line(
                from.x, from.y,
                to.x, to.y,
                thickness,
                std::mem::transmute(color)
            )
        }
    }
}