#ifndef PONI_H
#define PONI_H

#include <stdint.h>
#include <stdio.h>

typedef float   ps_float;
typedef int32_t ps_int;

static inline
void
ps_print_int(ps_int i) {
	printf("%d", i);
}

static inline
void
ps_print_float(float f) {
	printf("%f", f);
}

static inline
void
ps_println(void) {
	putc('\n', stdout);
}

#endif