#include <stdio.h>

#include <dlfcn.h>
#include <unistd.h>

#define PONI_HOT_IMPLEMENTATION
#include "../../poni/poni_hot.h"

static void tick_fn_null(void* closure) {}

static void (*tick_fn)(void*) = tick_fn_null;

static void
hot_event_trigger(struct poni_hot_context *ctx, enum poni_hot_event evt) {
    if(evt == PONI_HOT_DYNLIB_RELOADED) {
        puts("-- reloaded dynamic library --");
        // Look up the tick function whenever we're reloaded.
        tick_fn = poni_hot_lookup_fn(ctx, "f_tick");
        if(!tick_fn) {
            tick_fn = tick_fn_null;
        }
    }
}

int
main(int argc, char **argv) {
    struct poni_hot_context hot;
    poni_hot_init(&hot);

    hot.dynlib_path = "./game.so";
    hot.event_trigger = hot_event_trigger;

    // Only reload occasionally. (This is especially funny here...)
    int reload_countdown = 0;
    const int reload_rate = 2;

    for(;;) {
        poni_hot_poll(&hot);

        // Tick the game.
        tick_fn(NULL);

        usleep(100000);
    }
}