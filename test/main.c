#include <stdint.h>
#include <stddef.h>

struct poni_gc;
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

void* poni_gc_alloc(struct poni_gc_context *ctx, size_t size);
void  poni_gc_mark(struct poni_gc *gc, void *object);
struct poni_gc *poni_gc_spawn();
void poni_gc_poll_slow(struct poni_gc_context *ctx);

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

struct object1*
mk_obj1(struct poni_gc_context *ctx, int x, int y) {
    struct object1 *obj = poni_gc_alloc(ctx, sizeof(struct object1));
    obj->x = x;
    obj->y = y;
    return obj;
}

struct object2*
mk_obj2(struct poni_gc_context *ctx, struct object1 *o1, struct object1 *o2) {
    struct object2 *obj = poni_gc_alloc(ctx, sizeof(struct object2));
    obj->ptr1 = o1;
    obj->ptr2 = o2;
    return obj;
}

void
myfun(struct poni_gc_context *ctx) {
    union {
        struct poni_gc_frame gc_frame;
        struct {
            struct poni_gc_frame *gc_prev;
            uint64_t pointer_count;
            void *obj1_1;
            void *obj1_2;
        };
    } frame;
    frame.gc_prev = ctx->frame;
    ctx->frame = &frame.gc_frame;

    frame.obj1_1 = mk_obj1(ctx, 10, 20);
    frame.obj1_2 = mk_obj1(ctx, 30, 40);

    ctx->frame = frame.gc_prev;
}

int
main(int argc, char **argv) {

}