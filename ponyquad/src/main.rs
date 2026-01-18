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

fn rebuild(target_path: &str) {
    let mut process = Command::new("make")
        .arg(format!("OUTLIBNAME={}", target_path))
        .arg("hot-reload")
        .spawn().unwrap();

    process.wait().unwrap();
}

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

    let mut dynlib_idx: u32 = 1;
    let mut dynlib_name = format!("./tmp-script-0.so");

    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = notify::recommended_watcher(tx).unwrap();

    watcher.watch(Path::new("."), RecursiveMode::Recursive).unwrap();

    loop {
        clear_background(RED);

        draw_line(40.0, 40.0, 100.0, 200.0, 15.0, BLUE);
        draw_rectangle(screen_width() / 2.0 - 60.0, 100.0, 120.0, 60.0, GREEN);

        draw_text("Hello, Macroquad!", 20.0, 20.0, 30.0, DARKGRAY);

        f_update(ctx, ptr::null());

        if fs::exists(&dynlib_name).unwrap_or(false) {
            eprintln!("--- reloading script ---");
            library = unsafe { libloading::Library::new(&dynlib_name).unwrap() };
            f_update = unsafe { library.get("f_update").unwrap() };

            gc_visit = unsafe { library.get("poni_gc_visit_object").unwrap() };
            gc_roots = unsafe { library.get("poni_gc_visit_roots").unwrap() };
            gc_size = unsafe { library.get("poni_gc_get_allocation_size").unwrap() };
            poniescript_gc::load_gc_functions(*gc_visit, *gc_roots, *gc_size);

            // I think this is Linux-specific. Windows won't be happy with this.
            // We might want to do something like generate a unique name for
            // the library every time? Not sure how that would work.
            let _ = fs::remove_file(&dynlib_name);

            dynlib_idx += 1;
            dynlib_name = format!("./tmp-script-{}.so", dynlib_idx);
        }

        if let Ok(event) = rx.try_recv() {
            if let Ok(event) = event {
                if !event.kind.is_access() {
                    // let mut any_is_non_so = false;
                    // for path in event.paths {
                    //     eprintln!("path = {}", path.display());
                    //     if path.extension() != Some(OsStr::new("so")) {
                    //         any_is_non_so = true;
                    //         break;
                    //     }
                    // }
                    // if any_is_non_so {
                    //     // We have a filesystem event. Run the rebuild command.
                    //     rebuild(&dynlib_name);
                    // }

                    // It's not sufficient to just check whether the path is
                    // a non-.so, because all sorts of files end up being written
                    // when we compile.
                    //
                    // Instead, I guess let's just filter down to paths we care
                    // about. This is anything ending in .poni, .toml, or maybe
                    // also asset files; maybe we could keep track of every asset
                    // path we've touched and check those here.

                    let mut we_care = false;
                    for path in event.paths {
                        let interesting = match path.extension().and_then(|e| e.to_str()) {
                            Some("poni") => true,
                            Some("toml") => true,
                            _ => false,
                        };

                        if interesting {
                            we_care = true;
                        }
                    }

                    if we_care {
                        rebuild(&dynlib_name);
                    }
                }
            }
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