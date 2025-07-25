#include <stdio.h>

#include <dlfcn.h>
#include <unistd.h>

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
maybe_reload() {
    const char *check_path = dynlib_paths[next_path];

    // Try simply loading the next library, and unloading
    // the previous one if we succeed.
    //
    // This might not work later because they have the same
    // symbols. I guess we'll see.

    void *new_library = dlopen(check_path, RTLD_LAZY | RTLD_LOCAL);
    //printf("check: %s -> %p\n", check_path, new_library);
    // No new library? We're done.
    if(!new_library) return;

    // Clean up the old library.
    if(loaded_library) {
        dlclose(loaded_library);
        loaded_library = NULL;
    }

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
        tick_fn = new_tick;
    }

    // Always run poni_init.
    //
    // This is so we call poni_init_strings.
    //
    // What we really should do is have something like poni_init_internal()
    // and poni_init_globals() and so forth, and only run the ones that need
    // to be run on hot-reload, on hot-reload.
    //
    // NOTE: We would probably re-run the main init function on game "launch."
    void *init = dlsym(new_library, "poni_init");
    if(init) {
        void (*poni_init)(void) = init;
        poni_init();
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
    const int reload_rate = 5;

    for(;;) {
        // Reload game if there is one.
        reload_countdown += 1;
        if(reload_countdown >= reload_rate) {
            reload_countdown -= reload_rate;
            maybe_reload();
        }

        // Tick the game.
        tick_fn(NULL);
    }
}