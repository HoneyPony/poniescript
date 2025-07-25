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

#endif /* PONI_HOT_IMPLEMENTATION */

#endif