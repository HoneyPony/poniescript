#include <stdint.h>
#include <stdio.h>
#include <time.h>

// 7 arguments is when we should start spilling to stack on x86. So, in this
// case do_thing_2arg should spill while do_thing_mem shouldn't. Does that make
// 2arg on par with mem? If it's still faster, that should indicate that the
// 2-arg setup is better.
uint64_t
do_thing_2arg(void *context, uint64_t self, uint64_t a, uint64_t b, uint64_t c, uint64_t d, uint64_t e) {
    if(self <= 1) return self;

    a += 1;
    b += 2;
    c += 3;
    d += 4;
    e += 5;

    return do_thing_2arg(context, self - 1, a, b, c, d, e) + do_thing_2arg(context, self - 2, a, b, c, d, e) + a + b + c + d + e;
}

struct ps_context {
    uint64_t self;
};

uint64_t
do_thing_mem(struct ps_context *context, uint64_t a, uint64_t b, uint64_t c, uint64_t d, uint64_t e) {
    uint64_t self = context->self;
    if(self <= 1) return self;

    a += 1;
    b += 2;
    c += 3;
    d += 4;
    e += 5;

    context->self = self - 1;
    uint64_t tmp = do_thing_mem(context, a, b, c, d, e);
    context->self = self - 2;
    return tmp + do_thing_mem(context, a, b, c, d, e) + a + b + c + d + e;
}

#define VAL 43

void
show_time(const char *label, clock_t start, clock_t end) {
    clock_t time = end - start;
    double time_d = (double)time;
    double time_sec = time_d / (double)CLOCKS_PER_SEC;
    double time_ms = (time_d * 1000) / (double)CLOCKS_PER_SEC;
    printf("%s: %lu total (%f sec, %f ms)\n", label, time, time_sec, time_ms);
}

int
main() {
    struct ps_context ctx = { .self = VAL };

    clock_t start_2arg = clock();
    do_thing_2arg(NULL, VAL, 0, 0, 0, 0, 0);
    clock_t end_2arg = clock();

    clock_t start_mem = clock();
    do_thing_mem(&ctx, 0, 0, 0, 0, 0);
    clock_t end_mem = clock();

    show_time("2arg", start_2arg, end_2arg);
    show_time("mem", start_mem, end_mem);
}