#ifndef PONI_GC_H
#define PONI_GC_H

#include "poni_cstd.h"

// TCC unfortunately does not define STDC_NO_ATOMICS, even though it doesn't
// support atomics.
//
// Maybe we should just use has_include(stdatomic)?
#if defined(__STDC_NO_ATOMICS__) || defined(__TINYC__)

// If we don't have atomics support, i.e. tcc -- we still assume that the compiler
// will probably compile our code correctly (and you should use a compiler that
// does support atomics for release builds.)

extern uint64_t poni_gc_flags;

// We assume that on, e.g., x86, this will be equivalent to a relaxed load.
#define PONI_GC_READ_FLAGS (poni_gc_flags)

#else



extern _Atomic uint64_t poni_gc_flags;

// When we have atomics available, explicitly use a relaxed load; on most
// platforms, this really should just be equivalent to reading from a non-
// _Atomic pointer.
#define PONI_GC_READ_FLAGS atomic_load_explicit(&poni_gc_flags, memory_order_relaxed)

#endif

/** Opaque handle to Rust struct. */
struct poni_gc;
/** Opaque handle to Rust struct. */
struct poni_gc_handle;
/** Opaque handle to Rust struct. */
struct poni_gc_shared;

struct poni_gc_frame {
    struct poni_gc_frame *prev;
    const char           *fn_name;
    uint64_t              pointer_count;
    void                 *ptrs[];
};

struct poni_gc_context {
    struct poni_gc_frame  *frame;
    struct poni_gc_shared *shared;
};

#define PONI_GC_FLAG_SCAN 2
#define PONI_GC_FLAG_HANDOFF_ALLOCS  1

#define PONI_GC_REQUEST_COLLECT 1
#define PONI_GC_REQUEST_SHUTDOWN 2

#ifdef __GNUC__
#define PONI_UNLIKELY(expr) __builtin_expect(!!(expr), 0)
#else
#define PONI_UNLIKELY(expr) (expr)
#endif

#define PONI_WRITE_BARRIER(ptr) \
do { \
    if(poni_gc_is_marking && (ptr) && !((*(uint64_t*)ptr) & 1)) { \
        poni_gc_mark_from_anywhere(ptr); \
    } \
} while(0)

#define PONI_GC_SAFEPOINT(ctx) \
do { \
    if(PONI_UNLIKELY(PONI_GC_READ_FLAGS & (PONI_GC_FLAG_HANDOFF_ALLOCS | PONI_GC_FLAG_SCAN))) { \
        poni_gc_poll_slow(ctx); \
    } \
} while(0)

void* poni_gc_alloc(struct poni_gc_context *ctx, size_t size);

static inline void*
poni_gc_alloc_tagged(struct poni_gc_context *ctx, size_t size, uint64_t tag) {
    void *ptr = poni_gc_alloc(ctx, size);
    uint64_t *word = ptr;
    *word = tag;
    return ptr;
}

static inline void*
poni_gc_realloc(struct poni_gc_context *ctx, void *old, size_t new_size) {
    // At this time, there is no way to actually realloc an allocation in the
    // GC. So, instead just allocate a new one, copy stuff over, and then
    // let the GC handle the old allocation.
    void *newmem = poni_gc_alloc(ctx, new_size);
    memcpy(newmem, old, new_size);
    return newmem;
}

void poni_gc_mark(struct poni_gc *gc, void *object);
void poni_gc_poll_slow(struct poni_gc_context *ctx);
void poni_gc_poll_until_cycle_finished(struct poni_gc_context *ctx);

struct poni_gc_handle *poni_gc_spawn();

/**
 * Shutdown and then join the GC thread, using the handle. Every other thread
 * that is using the GC must have also shut down, otherwise this will block
 * until they do.
 * 
 * If this is being called from a thread with a poni_gc_context, provide the
 * context so that it can participate in safepoints, in case the GC is currently
 * collecting (or if you trigger a collection before joining).
 */
void poni_gc_join(struct poni_gc_handle *handle, struct poni_gc_context *maybe_spin_ctx);
void poni_gc_send_request(struct poni_gc_handle *handle, uint64_t request);
struct poni_gc_context *poni_gc_create_context_for_existing(struct poni_gc_handle *handle);

void poni_gc_free_context(struct poni_gc_context *ctx);
void poni_gc_free_handle(struct poni_gc_handle *handle);

#endif