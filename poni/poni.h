#ifndef PONI_H
#define PONI_H

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define PS_TAG_STRING 1

typedef float   ps_float;
typedef int32_t ps_int;

struct ps_object;
struct ps_string;

typedef struct ps_object {
	uint64_t todo;
} ps_object;

typedef struct ps_str {
	ps_object object;

	// TODO: ps_int?
	size_t length;

	char contents[];
} ps_str;

static inline
void _Noreturn
ps_fatal_error(const char *message) {
	printf("fatal error: %s\n", message);
	exit(1);
}

// This method is "infallible:" Either it allocates the requested memory, or
// the program will exit.
static inline
void*
ps_gc_must_calloc(size_t bytes, uint64_t tag) {
	// TODO: Implement GC
	void *result = calloc(bytes, 1);
	if(!result) {
		ps_fatal_error("infallible calloc could not allocate");
	}

	ps_object *header = result;
	header->todo = tag;

	return result;
}

static inline
ps_str*
ps_str_from_literal_size(const char *input, size_t length) {
	size_t bytes = sizeof(ps_str) + ((length + 1) * sizeof(char));
	ps_str *str = ps_gc_must_calloc(bytes, PS_TAG_STRING);

	memcpy(str->contents, input, length);
	str->contents[length] = '\0';
	str->length = length;

	return str;
}

#define ps_str_from_literal(lit) ps_str_from_literal_size(lit, sizeof(lit))

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
ps_print_str(const ps_str *str) {
	// TODO: Should the NULL check be part of the print() codegen?
	if(str) {
		printf("%s", str->contents);
	}
}

static inline
void
ps_println(void) {
	putc('\n', stdout);
}

#endif