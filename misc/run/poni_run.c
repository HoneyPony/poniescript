#include <stdio.h>

#include <dlfcn.h>
#include <unistd.h>

#define PONI_HOT_IMPLEMENTATION
#include "../../poni/poni_hot.h"

static void tick_fn_null(void* closure) {}

static void (*tick_fn)(void*) = tick_fn_null;

static const char *dynlib_paths[] = {
    "./game-a.so",
    "./game-b.so",
};

// Next path from dynlib_paths
static int next_path = 0;
// Whether this is the first ever load
static int first_load = 1;

void *loaded_library = NULL;

static void
run_initfn(void *library, const char *fnname) {
    void *init = dlsym(library, fnname);
    if(init) {
        void (*initfn)(void) = init;
        initfn();
    }
}

static void
maybe_reload() {
    const char *check_path = dynlib_paths[next_path];

    // Try simply loading the next library, and unloading
    // the previous one if we succeed.
    //
    // This might not work later because they have the same
    // symbols. I guess we'll see.

    void *new_library = dlopen(check_path, RTLD_NOW | RTLD_LOCAL);
    //printf("check: %s -> %p\n", check_path, new_library);
    // No new library? We're done.
    if(!new_library) return;

    printf("-- reload: %s --\n", check_path);

    // Clean up the old library.
    if(loaded_library) {
        dlclose(loaded_library);
        loaded_library = NULL;
    }

    // In order to force the reload to occur, dlopen the new library a second
    // time.
    void *handle2 = dlopen(check_path, RTLD_NOW | RTLD_LOCAL);
    if(!handle2) {
        puts("-- error: failed to properly load new library --");
        exit(1);
    }
    // Drop extra handle
    dlclose(handle2);

    // Delete the new library.
    unlink(check_path);

    // Try to find a tick function in the new library. If there is no such function,
    // revert to the tick_fn_null function.
    void *new_tick = dlsym(new_library, "f_tick");
    
    //printf("loaded %s new_tick = %p\n", check_path, new_library);
    if(!new_tick) {
        tick_fn = tick_fn_null;
    }
    else {
        printf("-- located tick fn: %p\n", new_tick);
        tick_fn = new_tick;
    }

    // Always run poni_init_strings then poni_init_globals.
    // 
    // Run poni_init() on first load.
    //
    // NOTE: We would probably re-run the main init function on game "launch."
    run_initfn(new_library, "poni_init_strings");
    run_initfn(new_library, "poni_init_globals");
    if(first_load) {
        run_initfn(new_library, "poni_init");
        first_load = 0;
    }

    // This is now the loaded library.
    loaded_library = new_library;

    // Advance to the next option.
    next_path = (next_path + 1) % 2;
}

int
main(int argc, char **argv) {
    // Load initial game.
    maybe_reload();

    // Only reload occasionally. (This is especially funny here...)
    int reload_countdown = 0;
    const int reload_rate = 2;

    for(;;) {
        // Reload game if there is one.
        reload_countdown += 1;
        if(reload_countdown >= reload_rate) {
            reload_countdown -= reload_rate;
            maybe_reload();
        }

        // Tick the game.
        tick_fn(NULL);

        usleep(100000);
    }
}