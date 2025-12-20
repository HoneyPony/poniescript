#include <stdio.h>
#include <time.h>
#include <stdint.h>

struct context {
    int value;
};

int test1_tl(int, void*);
int test2_tl(int, void*);
void init_tl(struct context*);
int test1_param(struct context*, int, void*);
int test2_param(struct context*, int, void*);

#define DO_LOOPS(inner, label) { \
    clock_t start = clock(); \
    for(uint64_t j = 0; j < iters; ++j) { \
        inner ; \
    } \
    clock_t end = clock(); \
    double time = (double)(end - start) / (double)CLOCKS_PER_SEC; \
    printf("%s: time: %f\n", label, time); \
}

int
main() {
    struct context ctx = { .value = 30 };
    init_tl(&ctx);

    uint64_t runs = 5;
    uint64_t iters = 1000000;

    const int exponent = 4;

    for(uint64_t i = 0; i < runs; ++i) {
        DO_LOOPS(test1_tl(exponent, NULL)         , "thread_local");
        DO_LOOPS(test1_param(&ctx, exponent, NULL), "parameter   ");
    }
}