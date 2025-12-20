struct context {
    int value;
};

int
test2_param(struct context *gc, int depth, void *closure) {
    int ctx = gc->value;
    if(depth <= 0) {
        return ctx + 5;
    }
    return ctx + test2_param(gc, depth - 1, closure);
}

int
test1_param(struct context *gc, int depth, void *closure) {
    if(depth <= 0) {
        return test2_param(gc, 7, closure);
    }
    return test1_param(gc, depth - 1, closure) + test1_param(gc, depth - 2, closure) + 2;
}

int
test3_param(struct context *gc, int depth, void *closure) {
    if(depth <= 0) {
        return 0;
    }

    int value = 0;

    if(depth % 64 == 0) {
        value += gc->value;
    }

    value += test3_param(gc, depth - 1, closure) + depth;

    return value;
}