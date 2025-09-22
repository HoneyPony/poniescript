#include <stdint.h>
#include <stddef.h>
#include <stdatomic.h>
#include <stdio.h>
#include <time.h>

struct poni_gc;
struct poni_gc_handle;
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

#define PONI_UNLIKELY(expr) __builtin_expect(!!(expr), 0)

#define PONI_GC_SAFEPOINT(ctx) \
do { \
    if(PONI_UNLIKELY(atomic_load_explicit(&poni_gc_flags, memory_order_relaxed) & (PONI_GC_FLAG_NOP | PONI_GC_FLAG_SCAN))) { \
        poni_gc_poll_slow(ctx); \
    } \
} while(0)

void* poni_gc_alloc(struct poni_gc_context *ctx, size_t size);
void  poni_gc_mark(struct poni_gc *gc, void *object);
void poni_gc_poll_slow(struct poni_gc_context *ctx);
void poni_gc_poll_until_cycle_finished(struct poni_gc_context *ctx);

struct poni_gc_handle *poni_gc_spawn();
void poni_gc_join(struct poni_gc_handle *handle);
void poni_gc_send_request(struct poni_gc_handle *handle, uint64_t request);
struct poni_gc_context *poni_gc_create_context_for_existing(struct poni_gc_handle *handle);

void poni_gc_free_context(struct poni_gc_context *ctx);
void poni_gc_free_handle(struct poni_gc_handle *handle);

struct object1 {
    uint64_t tag;
    int x;
    int y;
};

struct object2 {
    uint64_t tag;
    struct object1 *ptr1;
    struct object1 *ptr2;
};



#define TAG_OBJ1 0x2
#define TAG_OBJ2 0x4

void
poni_gc_visit_object(struct poni_gc *gc, void *object) {
    uint64_t tag = *(uint64_t*)object;
    switch(tag & 0xFE) {
        case TAG_OBJ1: break;
        case TAG_OBJ2: {
            struct object2 *obj = object;
            poni_gc_mark(gc, obj->ptr1);
            poni_gc_mark(gc, obj->ptr2);
        }
    }
}

void
poni_gc_visit_roots(struct poni_gc *gc) {

}

size_t
poni_gc_get_allocation_size(void *object) {
    uint64_t tag = *(uint64_t*)object;
    switch(tag & 0xFE) {
        case TAG_OBJ1: return sizeof(struct object1);
        case TAG_OBJ2: return sizeof(struct object2);
        default: return 0;
    }
}

struct object1*
mk_obj1(struct poni_gc_context *ctx, int x, int y) {
    struct object1 *obj = poni_gc_alloc(ctx, sizeof(struct object1));
    obj->tag |= TAG_OBJ1;
    obj->x = x;
    obj->y = y;
    return obj;
}

struct object2*
mk_obj2(struct poni_gc_context *ctx, struct object1 *o1, struct object1 *o2) {
    struct object2 *obj = poni_gc_alloc(ctx, sizeof(struct object2));
    obj->tag |= TAG_OBJ2;
    obj->ptr1 = o1;
    obj->ptr2 = o2;
    return obj;
}

#define PONI_FRAME(ptr_count, ...) \
union { \
    struct poni_gc_frame gc_frame; \
    struct { \
        struct poni_gc_frame *gc_prev; \
        uint64_t gc_count; \
        __VA_ARGS__ \
    }; \
} frame; \
frame.gc_count = ptr_count; \
frame.gc_prev = ctx->frame; \
ctx->frame = &frame.gc_frame;

#define PONI_RETURN(expr) \
do { \
    ctx->frame = frame.gc_prev; \
    return expr; \
} while(0)

#define PONI_EXIT() \
do { \
    ctx->frame = frame.gc_prev; \
} while(0)

struct object2*
myfun(struct poni_gc_context *ctx) {
    union {
        struct poni_gc_frame gc_frame;
        struct {
            struct poni_gc_frame *gc_prev;
            uint64_t pointer_count;
            void *obj1_1;
            void *obj1_2;
            void *obj2;
        };
    } frame;
    frame.pointer_count = 2;
    frame.gc_prev = ctx->frame;
    ctx->frame = &frame.gc_frame;

    frame.obj1_1 = mk_obj1(ctx, 10, 20);
    frame.obj1_2 = mk_obj1(ctx, 30, 40);
    frame.obj2 = mk_obj2(ctx, frame.obj1_1, frame.obj1_2);

    ctx->frame = frame.gc_prev;
    return frame.obj2;
}

void
benchmark_safepoint(struct poni_gc_context *ctx) {
    clock_t start = clock();
    const int iters = 10000000;
    for(int i = 0; i < iters; ++i) {
        PONI_GC_SAFEPOINT(ctx);
    }

    clock_t end = clock();
    clock_t time = end - start;
    double total_ms = (time / (double)CLOCKS_PER_SEC) * 1000.0;
    double per_iter_ns = (total_ms / (double)iters) * 10000000.0;
    printf("safepoint time: %fms total, %fns per-iter\n", total_ms, per_iter_ns);
}

void
do_loop_benchmark(struct poni_gc_handle *handle, struct poni_gc_context *ctx, int do_safepoints) {
    poni_gc_send_request(handle, PONI_GC_REQUEST_COLLECT);
    clock_t loop_start = clock();

    PONI_FRAME(1, struct object2 *myobj;)
    for(int i = 0; i < 1000000; ++i) {
        frame.myobj = myfun(ctx);

        if(do_safepoints) PONI_GC_SAFEPOINT(ctx);
    }

    clock_t loop_end = clock();
    const char *text = do_safepoints ? "safepoints" : "just alloc";
    printf("%s: %fms\n", text, 1000.0 * (double)(loop_end - loop_start) / (double)CLOCKS_PER_SEC);

    for(int i = 0; i < 500; ++i) {
        PONI_GC_SAFEPOINT(ctx);
        usleep(20);
    }

    //poni_gc_send_request(handle, PONI_GC_REQUEST_COLLECT);
    //poni_gc_poll_until_cycle_finished(ctx);

    PONI_EXIT();
}

int
main(int argc, char **argv) {
    struct poni_gc_handle *handle = poni_gc_spawn();
    struct poni_gc_context *ctx = poni_gc_create_context_for_existing(handle);

    for(int j = 0; j < 2; ++j) {
        for(int i = 0; i < 20; ++i) {
            do_loop_benchmark(handle, ctx, j & 1);
        }
    }

    printf("testgc: joining gc\n");

    poni_gc_join(handle);

    poni_gc_free_context(ctx);
    poni_gc_free_handle(handle);
}