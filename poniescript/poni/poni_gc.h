#ifndef PONI_GC_H
#define PONI_GC_H

#include <stdint.h>
#include <stddef.h>
#include <stdatomic.h>
#include <stdio.h>
#include <time.h>

/** Opaque handle to Rust struct. */
struct poni_gc;
/** Opaque handle to Rust struct. */
struct poni_gc_handle;
/** Opaque handle to Rust struct. */
struct poni_gc_shared;

struct poni_gc_frame {
    struct poni_gc_frame *prev;
    uint64_t              pointer_count;
    void                 *ptrs[];
};

struct poni_gc_context {
    struct poni_gc_frame  *frame;
    struct poni_gc_shared *shared;
};

extern _Atomic uint64_t poni_gc_flags;
#define PONI_GC_FLAG_SCAN 2
#define PONI_GC_FLAG_NOP  1

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
    if(PONI_UNLIKELY(atomic_load_explicit(&poni_gc_flags, memory_order_relaxed) & (PONI_GC_FLAG_NOP | PONI_GC_FLAG_SCAN))) { \
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

void  poni_gc_mark(struct poni_gc *gc, void *object);
void poni_gc_poll_slow(struct poni_gc_context *ctx);
void poni_gc_poll_until_cycle_finished(struct poni_gc_context *ctx);

struct poni_gc_handle *poni_gc_spawn();
void poni_gc_join(struct poni_gc_handle *handle);
void poni_gc_send_request(struct poni_gc_handle *handle, uint64_t request);
struct poni_gc_context *poni_gc_create_context_for_existing(struct poni_gc_handle *handle);

void poni_gc_free_context(struct poni_gc_context *ctx);
void poni_gc_free_handle(struct poni_gc_handle *handle);

#endif