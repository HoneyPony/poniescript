#include <threads.h>

struct context {
    int value;
};

thread_local struct context *gc_context = NULL;

int
test2_tl(int depth, void *closure) {
    int ctx = gc_context->value;
    if(depth <= 0) {
        return ctx + 5;
    }
    return ctx + test2_tl(depth - 1, closure);
}

int
test1_tl(int depth, void *closure) {
    if(depth <= 0) {
        return test2_tl(7, closure);
    }
    return test1_tl(depth - 1, closure) + test1_tl(depth - 2, closure) + 2;
}

// test3: Trying to lean a bit more towards thread_locals; we only read from
// the variable relatively occasionally.
int
test3_tl(int depth, void *closure) {
    if(depth <= 0) {
        return 0;
    }

    int value = 0;

    if(depth % 64 == 0) {
        value += gc_context->value;
    }

    value += test3_tl(depth - 1, closure) + depth;

    return value;
}

void
init_tl(struct context *ctx) {
    gc_context = ctx;
}