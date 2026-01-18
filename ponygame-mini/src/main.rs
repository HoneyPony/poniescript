use macroquad::prelude::*;

use std::{ffi::c_void, fs, ptr};
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

    let mut library = unsafe { libloading::Library::new("./game-script.so").unwrap() };
    let mut f_update: libloading::Symbol<'_, fn(&mut GcContext, *const c_void)> = unsafe { library.get("f_update").unwrap() };

    let mut gc_visit = unsafe { library.get("poni_gc_visit_object").unwrap() };
    let mut gc_roots = unsafe { library.get("poni_gc_visit_roots").unwrap() };
    let mut gc_size = unsafe { library.get("poni_gc_get_allocation_size").unwrap() };
    poniescript_gc::load_gc_functions(*gc_visit, *gc_roots, *gc_size);

    let dynlib_names = ["./game-script-A.so", "./game-script-B.so"];
    let mut dynlib_idx = 0;

    loop {
        clear_background(RED);

        draw_line(40.0, 40.0, 100.0, 200.0, 15.0, BLUE);
        draw_rectangle(screen_width() / 2.0 - 60.0, 100.0, 120.0, 60.0, GREEN);

        draw_text("Hello, Macroquad!", 20.0, 20.0, 30.0, DARKGRAY);

        f_update(ctx, ptr::null());

        if fs::exists(dynlib_names[dynlib_idx]).unwrap_or(false) {
            eprintln!("--- reloading script ---");
            library = unsafe { libloading::Library::new(dynlib_names[dynlib_idx]).unwrap() };
            f_update = unsafe { library.get("f_update").unwrap() };

            gc_visit = unsafe { library.get("poni_gc_visit_object").unwrap() };
            gc_roots = unsafe { library.get("poni_gc_visit_roots").unwrap() };
            gc_size = unsafe { library.get("poni_gc_get_allocation_size").unwrap() };
            poniescript_gc::load_gc_functions(*gc_visit, *gc_roots, *gc_size);

            // I think this is Linux-specific. Windows won't be happy with this.
            // We might want to do something like generate a unique name for
            // the library every time? Not sure how that would work.
            let _ = fs::remove_file(dynlib_names[dynlib_idx]);

            dynlib_idx = (dynlib_idx + 1) % dynlib_names.len();
        }

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