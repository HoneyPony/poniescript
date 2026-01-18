//! Hot-reloading support for Ponyquad.
//! 
//! It is important that we compile this out when NOT hot-reloading.

use std::ffi::c_void;

/// Helper module for the "watching the filesystem" part of the task.
/// 
/// It is OK if we are unable to create the filesystem watcher for any reason.
/// The game can still run.
mod watcher {
    use std::{path::Path, sync::mpsc};

    use notify::{Event, RecommendedWatcher, RecursiveMode};

    pub struct Watcher {
        rx: mpsc::Receiver<notify::Result<Event>>,
        watcher: RecommendedWatcher,
    }

    impl Watcher {
        pub fn new() -> Option<Self> {
            use notify::Watcher;

            let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
            let mut watcher = notify::recommended_watcher(tx).ok()?;

            watcher.watch(Path::new("."), RecursiveMode::Recursive).ok()?;

            Some(Self {
                rx,
                watcher
            })
        }

        /// Helper function for poll(). 
        ///
        /// Repeatedly calls try_recv() until it is out of events. Returns true
        /// if we should fire the callback, and false otherwise.
        fn poll_all(&self) -> bool {
            let mut should_call = false;

            while let Ok(event) = self.rx.try_recv() {
                if let Ok(event) = event {
                    if !event.kind.is_access() {
                        // It's not sufficient to just check whether the path is
                        // a non-.so, because all sorts of files end up being written
                        // when we compile.
                        //
                        // Instead, I guess let's just filter down to paths we care
                        // about. This is anything ending in .poni, .toml, or maybe
                        // also asset files; maybe we could keep track of every asset
                        // path we've touched and check those here.

                        for path in event.paths {
                            let interesting = match path.extension().and_then(|e| e.to_str()) {
                                Some("poni") => true,
                                Some("toml") => true,
                                _ => false,
                            };

                            if interesting {
                                should_call = true;
                                break;
                            }
                        }

                        // Note that even if should_call is true, we have to 
                        // keep polling, so that we can flush all the events out
                        // and avoid redundant rebuilds.
                    }
                }
            }

            return should_call;
        }

        pub fn poll<F: Fn()>(&self, callback: F) {
            if self.poll_all() {
                callback();
            }
        }
    }
}

#[cfg(feature = "hotreload")]
use libloading::Library;
use poniescript_gc::{Gc, GcContext};

#[cfg(feature = "hotreload")]
pub struct HotReload {
    update_fn: fn(&mut GcContext, *mut c_void),
    library: Library,

    next_tmp_lib: String,
    // Used to generate the next_tmp_lib path.
    next_tmp_idx: u32,

    /// The filesystem watcher we use to try to rebuild the code.
    /// 
    /// This part is optional, but we would prefer it is available.
    watcher: Option<watcher::Watcher>,
}

#[cfg(not(feature = "hotreload"))]
pub struct HotReload {}

#[cfg(feature = "hotreload")]
pub fn call_update(ctx: &mut GcContext, hot: &mut HotReload) {
    (hot.update_fn)(ctx, std::ptr::null_mut());
}

#[cfg(not(feature = "hotreload"))]
pub fn call_update(ctx: &mut GcContext, hot: &mut HotReload) {
    unsafe extern "C" { fn f_update(_ctx: &mut GcContext, _closure: *mut c_void); }

    unsafe { f_update(ctx, std::ptr::null_mut()); }
}

#[cfg(feature = "hotreload")]
fn reload_gc_functions(library: &Library) -> Option<()> {
    unsafe {
        let gc_visit = library.get("poni_gc_visit_object").ok()?;
        let gc_roots = library.get("poni_gc_visit_roots").ok()?;
        let gc_size = library.get("poni_gc_get_allocation_size").ok()?;
        poniescript_gc::load_gc_functions(*gc_visit, *gc_roots, *gc_size);
    }

    Some(())
}

fn call_init_hook(library: &Library, name: &str, gc: &mut GcContext) -> Option<()> {
    let hook: libloading::Symbol<fn(&mut GcContext)> = unsafe { library.get(name).ok()? };
    hook(gc);

    Some(())
}

#[cfg(feature = "hotreload")]
impl HotReload {
    pub fn new(game_script_path: &str, gc: &mut GcContext) -> Option<Self> {
        let library = unsafe { libloading::Library::new(game_script_path).ok()? };
        let update_fn: libloading::Symbol<'_, fn(&mut GcContext, *mut c_void)>
            = unsafe { library.get("f_update").ok()? };

        reload_gc_functions(&library)?;

        call_init_hook(&library, "poni_init_strings", gc)?;
        call_init_hook(&library, "poni_init_globals", gc)?;

        Some(Self {
            update_fn: *update_fn,
            library,

            // This will have to be different on Windows...?
            next_tmp_lib: format!("./tmp-script-0.so"),
            next_tmp_idx: 0,

            watcher: watcher::Watcher::new(),
        })
    }

    fn rebuild(&self) {
        use std::process::Command;

        let mut process = Command::new("make")
            .arg(format!("OUTLIBNAME={}", self.next_tmp_lib))
            .arg("hot-reload")
            .spawn().unwrap();

        process.wait().unwrap();
    }

    fn reload_lib_internal(&mut self, gc: &mut GcContext) -> Option<()> {
        unsafe {
            let new_lib = libloading::Library::new(&self.next_tmp_lib).ok()?;
            let update_fn = new_lib.get("f_update").ok()?;

            reload_gc_functions(&new_lib)?;

            call_init_hook(&new_lib, "poni_init_strings", gc)?;
            call_init_hook(&new_lib, "poni_init_globals", gc)?;
            
            // Success!

            // Note that we have to dereference the borrow here so that we can
            // move the library.
            self.update_fn = *update_fn;
            self.library = new_lib;
        }

        Some(())
    }

    /// Performs two tasks.
    /// 
    /// First, if we have any notifications from the filesystem watcher, we
    /// invoke the command to rebuild the PonieScript code.
    /// 
    /// Second, if the "temporary script library" that we expect to have built
    /// exists, we attempt to load it.
    pub fn poll(&mut self, gc: &mut GcContext) {
        use std::fs;

        if fs::exists(&self.next_tmp_lib).unwrap_or(false) {
            eprintln!("--- reloading script ---");

            match self.reload_lib_internal(gc) {
                Some(_) => eprintln!("--- successfully reloaded script ---"),
                None => eprintln!("--- failed to reload script ---"),
            }

            // I think this is Linux-specific. Windows won't be happy with this.
            // We might want to do something like generate a unique name for
            // the library every time? Not sure how that would work.
            let _ = fs::remove_file(&self.next_tmp_lib);

            self.next_tmp_idx += 1;
            self.next_tmp_lib = format!("./tmp-script-{}.so", self.next_tmp_idx);
        }

        if let Some(watcher) = self.watcher.as_ref() {
            watcher.poll(|| {
                self.rebuild();
            });
        }
    }
}

#[cfg(not(feature = "hotreload"))]
impl HotReload {
    #[inline(always)]
    pub fn new(game_script_path: &str, gc: &mut GcContext) -> Option<Self> {
        Some(Self {})
    }

    #[inline(always)]
    pub fn poll(&mut self, gc: &mut GcContext) {}
}