#ifndef PONI_HOT_H
#define PONI_HOT_H

/**
 * Support functions for hot-code reloading in PonieScript.
 * 
 * The most significant limitation of e.g. dlopen() is that global variables
 * do not keep their state when we reload the library.
 * 
 * As such, what we want to do is move their data to the heap, so that they
 * can be told where it is through some sort of communication with the hot
 * reload host.
 * 
 * This file declares and defines the host API. That is, the shared library
 * will want to *reference* functions from here, but they must be *defined*
 * in the hot-reload host (because, of course, the host is the one who is able
 * to keep stuff around).
 * 
 * In any case, #define PONI_HOT_IMPLEMENTATION when including this in the
 * host.
 */

#include <stddef.h>
#include <stdbool.h>

/**
 * Look up a global variable by C declaration and expected size in bytes.
 * 
 * If the variable did not yet exist, then *existed will be set to false, otherwise
 * *existed will be set to true. This allows the hot-reloaded code to only
 * initialize globals when they did not yet exist.
 * 
 * Note that this API will likely change to make the cdecl string more precise,
 * e.g. some kind of base64 encoding of the entire data structure. It may even 
 * evolve to allow us to upgrade variables between runs.
 */
void *poni_hot_lookup(const char *cdecl, size_t expected_bytes, bool *existed);

#ifdef PONI_HOT_IMPLEMENTATION

#include <string.h>
#include <stdlib.h>
#include <stdio.h>

#include <sys/inotify.h>
#include <sys/stat.h>

#include <dlfcn.h>

// For now, use a dead-simple linked list and direct string comparisons.
// This is very slow and inefficient. I am assuming we will eventually have
// a decent hash map structure for poni.h, in which case we can switch this
// to using that.
struct poni_hot_entry {
    struct poni_hot_entry *next;

    char *cdecl;
    size_t bytes;
    
    /**
     * The actual allocation for the global. Allows us to keep allocations
     * tighter.
     */
    char buf[];
};

static struct poni_hot_entry *poni_hot_list = NULL;

void*
poni_hot_lookup(const char *cdecl, size_t expected_bytes, bool *existed) {
    struct poni_hot_entry **entry_ptr = &poni_hot_list;
    while(*entry_ptr) {
        struct poni_hot_entry *entry = *entry_ptr;

        // Check for match
        if(expected_bytes == entry->bytes && !strcmp(entry->cdecl, cdecl)) {
            if(existed) *existed = true;

            return entry->buf;
        }

        entry_ptr = &entry->next;
    }

    if(existed) *existed = false;

    // Note: These will never be freed, so we don't need to use the GC.
    // That said, we might want to hook them into the GC somehow eventually.
    struct poni_hot_entry *new_entry = malloc(sizeof(*new_entry) + expected_bytes);
    if(!new_entry) { puts("poni_hot: couldn't allocate global"); exit(1); }

    new_entry->bytes = expected_bytes;
    new_entry->cdecl = strdup(cdecl);
    new_entry->next = NULL;

    if(!new_entry->cdecl) { puts("poni_hot: couldn't allocate global"); exit(1); }

    // Set head/next
    *entry_ptr = new_entry;

    return new_entry->buf;
}

enum poni_hot_event {
    PONI_HOT_DYNLIB_FAILED_LOAD,
    PONI_HOT_DYNLIB_RELOADED,

    PONI_HOT_FILE_CHANGED,
};

/**
 * Used to implement hot reloading functionality in an application (mainly
 * the PonyGame game engine).
 */
struct poni_hot_context {
    /** Path to read for the main dynamic library to reload. */
    const char *dynlib_path;

    /** Temporary paths to use for the library name. */
    const char *tmp_paths[2];

    /** Which temporary path is currently being used. */
    int tmp_path_idx;

    /** Whether to call poni_init when the library is loaded. */
    bool call_poni_init;

    /** Whether to disable the call to poni_init when it is called. */
    bool autodisable_poni_init;

    /** Function to call when something interesting happens. */
    void (*event_trigger)(struct poni_hot_context *ctx, enum poni_hot_event);

    /** Internal: Handle to dlopen() library. */
    void *dl_handle;

    /** Internal: Fd used to communicate with inotify */
    int inotify_fd;

    /** Internal: stat_buf tracking the dynamic library */
    struct stat stat_buf;
    int stat_err;
};

void*
poni_hot_lookup_fn(struct poni_hot_context *context, const char *fnname) {
    if(!context) return NULL;
    if(!context->dl_handle) return NULL;

    return dlsym(context->dl_handle, fnname);
}

/**
 * Invokes a symbol name that is a function of the form void (*fn)(void).
 */
void
poni_hot_invoke_oneshot(struct poni_hot_context *context, const char *fnname) {
    void *fn = poni_hot_lookup_fn(context, fnname);
    if(!fn) return;

    void (*thefn)(void) = fn;
    thefn();
}

void
poni_hot_poll_dynlib(struct poni_hot_context *context) {
    struct stat now_buf;
    int err = stat(context->dynlib_path, &now_buf);

    // No dynamic library -- give up.
    if(err < 0) {
        context->stat_err = err; // Keep track for next time.
        return;
    }

    bool reload = false;
    if(context->stat_err < 0) {
        // If the previous stat had an error and now we don't, then reload.
        reload = true;
    }
    // TODO: Consider comparing whole timespec.
    else if(context->stat_buf.st_mtime < now_buf.st_mtime) {
        reload = true;
    }

    // Note: Don't update the context-> values until we properly load another
    // library.

    if(!reload) {
        // No reload necessary - return.
        return;
    }

    // We can't just directly load from the dynlib_path, because the previous
    // library handle could still be open, in which case it will just return
    // the same one.
    //
    // Instead, we link the file to a temporary path, load it from that one,
    // and then unlink that path.

    const char *tmp_path = context->tmp_paths[context->tmp_path_idx];
    if(link(context->dynlib_path, tmp_path) < 0) {
        // Couldn't link; try again later.
        
        // TODO: First of all, is there any reason to think that by the time
        // we call dlopen() the link() will have actually occurred? Second,
        // it seems really annoying that we have to manually clean up the links
        // in case something breaks. Maybe we should have an option to unconditionally
        // unlink the hot-reload targets.
        return;
    }

    void *new_lib = dlopen(tmp_path, RTLD_NOW | RTLD_LOCAL);
    if(!new_lib) {
        unlink(tmp_path);
        if(context->event_trigger) context->event_trigger(context, PONI_HOT_DYNLIB_FAILED_LOAD);
        return;
    }

    // Okay, we loaded the library, close the old one.
    if(context->dl_handle) {
        dlclose(context->dl_handle);
        context->dl_handle = NULL;
    }

    // Update temporary paths and unlink the previous one.
    unlink(tmp_path);
    context->tmp_path_idx = (context->tmp_path_idx + 1) % 2;

    context->dl_handle = new_lib;

    // Run initialization oneshots.
    poni_hot_invoke_oneshot(context, "poni_init_strings");
    poni_hot_invoke_oneshot(context, "poni_init_globals");
    if(context->call_poni_init) {
        poni_hot_invoke_oneshot(context, "poni_init");
        if(context->autodisable_poni_init) {
            context->call_poni_init = false;
        }
    }

    // Now we are allowed to update the context stored values.
    context->stat_err = err;
    context->stat_buf = now_buf;

    if(context->event_trigger) context->event_trigger(context, PONI_HOT_DYNLIB_RELOADED);
}

void
poni_hot_poll_watched(struct poni_hot_context *context) {
    if(!context) return;
    if(context->inotify_fd < 0) return;

    char buf[4096];
    ssize_t bytes = read(context->inotify_fd, buf, 4096);

    if(bytes > 0) {
        // We read some inotify events. We actually really don't care what
        // they are. Just run the trigger.
        
        if(context->event_trigger) context->event_trigger(context, PONI_HOT_FILE_CHANGED);
    }
}

bool
poni_hot_init(struct poni_hot_context *context) {
    if(!context) return false;

    int fd = inotify_init1(IN_NONBLOCK);
    if(fd < 0) {
        return false;
    }

    context->dynlib_path = NULL;
    context->call_poni_init = true;
    context->autodisable_poni_init = true;
    context->event_trigger = NULL;
    context->dl_handle = NULL;
    context->inotify_fd = fd;

    context->tmp_paths[0] = "./__tmp_module_000001.so";
    context->tmp_paths[1] = "./__tmp_module_000002.so";
    context->tmp_path_idx = 0;

    // Perform initial library load.
    context->stat_err = -1;
    poni_hot_poll_dynlib(context);

    return true;
}

/**
 * Polls the context, and performs the following:
 * 1) Performs the dynlib-recompile trigger, if watched files have changed.
 * 2) Reloads the dynlib if it is newer than the previously loaded one.
 */
void
poni_hot_poll(struct poni_hot_context *context) {
    poni_hot_poll_dynlib(context);
    poni_hot_poll_watched(context);
}

/**
 * Watches a file for changes. Whenever a change is detected, the next time
 * poni_hot_poll is called, it will run the rebuild_trigger.
 */
bool
poni_hot_watch(struct poni_hot_context *context, const char *file_path) {
    // Only watch for modifications. Things like deletes will likely break the
    // project anyway (?)
    //
    // This may change as this gets more sophisticated.
    int err = inotify_add_watch(context->inotify_fd, file_path, IN_MODIFY);
    if(err < 0) { return false; }

    return true;
}

void
poni_hot_destroy(struct poni_hot_context *context) {
    if(context->inotify_fd >= 0) {
        close(context->inotify_fd);
        context->inotify_fd = -1;
    }
}

#endif /* PONI_HOT_IMPLEMENTATION */

#endif