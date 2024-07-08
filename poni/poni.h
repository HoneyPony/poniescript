#ifndef PONI_H
#define PONI_H

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <inttypes.h>

#define PS_TAG_STRCONST 1
#define PS_TAG_STR      2
#define PS_TAG_STRBUF   3

// Tag for closure captures that have no garbage collected data themselves.
#define PS_TAG_CLOSURE_NOGC 4

// Tag for closure objects.
#define PS_TAG_CLOSURE 5

typedef float   ps_float;
typedef int32_t ps_int;
typedef int8_t  ps_bool;

struct ps_object;
struct ps_str;
struct ps_strbuf;

typedef struct ps_object {
	uint64_t todo;
} ps_object;

typedef struct ps_str {
	ps_object object;

	// TODO: ps_int?
	size_t length;

	char contents[];
} ps_str;

typedef struct ps_strbuf {
	ps_object object;

	// Note: buffer->length == allocated, essentially
	ps_str *buffer;
	size_t length;
} ps_strbuf;

// Example closure:
// typedef struct some_closure {
// 	ps_object object;
// 	fun_ptr ptr;
// 	void *closure;
// 	ps_intval *val;
// }

typedef struct ps_intval {
	ps_object object;
	ps_int val;
} ps_intval;

typedef struct ps_floatval {
	ps_object object;
	ps_float val;
} ps_floatval;

typedef struct ps_boolval {
	ps_object object;
	ps_bool val;
} ps_boolval;

typedef struct ps_refval {
	ps_object object;
	void *val;
} ps_refval;

#ifdef __TINYC__
	#define PONI_NORETURN __attribute__((noreturn))
#else
	#define PONI_NORETURN _Noreturn
#endif

static inline
PONI_NORETURN void
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
	// Consideration: For realloc(), it would be helpful if it could extend
	// an existing allocation further in to the arena.
	//
	// This would be especially useful for str(), because if it has no
	// inner allocating expressions, it will be able to essentially extend
	// its buffer "for free" when needed. (And we could even make it so
	// that it computes all inner expressions first).
	void *result = calloc(bytes, 1);
	if(!result) {
		ps_fatal_error("infallible calloc could not allocate");
	}

	ps_object *header = result;
	header->todo = tag;

	return result;
}

static inline
void*
ps_gc_must_realloc(void *previous, size_t bytes) {
	void *result = realloc(previous, bytes);
	if(!result) {
		ps_fatal_error("infallible realloc could not allocate");
	}

	return result;
}

static inline
ps_str*
ps_str_from_literal_size(const char *input, size_t length) {
	size_t bytes = sizeof(ps_str) + ((length + 1) * sizeof(char));
	ps_str *str = ps_gc_must_calloc(bytes, PS_TAG_STRCONST);

	memcpy(str->contents, input, length);
	str->contents[length] = '\0';
	str->length = length;

	return str;
}

static inline
ps_str*
ps_str_from_alloc(size_t length) {
	// For from_alloc, do not add 1 to length.
	size_t bytes = sizeof(ps_str) + ((length) * sizeof(char));
	ps_str *str = ps_gc_must_calloc(bytes, PS_TAG_STRCONST);

	str->length = length;
	return str;
}

// Use sizeof(lit) - 1 because the length value does not include NUL terminator
#define ps_str_from_literal(lit) ps_str_from_literal_size(lit, (sizeof(lit) - 1))

static inline
ps_strbuf*
ps_strbuf_new(size_t prealloc) {
	ps_strbuf *result = ps_gc_must_calloc(sizeof(*result), PS_TAG_STRBUF);
	result->buffer = ps_str_from_alloc(prealloc);
	result->length = 0;

	return result;
}

static inline
void
ps_strbuf_reserve(ps_strbuf *buf, size_t needed) {
	size_t new_len = buf->buffer->length;
	needed = buf->length + needed;

	// Don't reallocate if it's already big enough
	if(new_len >= needed) { return; }

	while(new_len < needed) {
		new_len *= 2;
	}

	// Note that we are NOT including the NUL terminator here. That should
	// be included in the 'needed' value.
	size_t bytes = sizeof(ps_str) + ((new_len) * sizeof(char));

	buf->buffer = ps_gc_must_realloc(buf->buffer, bytes);
	buf->buffer->length = new_len;
}

static inline
void
ps_strfmt_int(ps_strbuf *buf, ps_int i) {
	// We will compare the snprintf() result against the total chars -1,
	// because snprintf() returns the length of everything BUT the NUL
	// terminator.
	size_t rem = (buf->buffer->length - buf->length) - 1;
	int needed = snprintf(buf->buffer->contents + buf->length, rem, "%d", i);

	if(rem < needed) {
		// If we didn't have enough room, we will reallocate and do the
		// snprintf() again. Reserve needed + 1 so that we include the NUL terminator.
		ps_strbuf_reserve(buf, needed + 1);

		// Do the snprintf again. The output should not change.
		// We will recompute rem, although it should be the case that
		// there's always enough room.
		rem = (buf->buffer->length - buf->length) - 1;
		snprintf(buf->buffer->contents + buf->length, rem, "%d", i);
	}

	// Finally, the length of the string should increase by needed.
	// Then, we should write a NUL terminator.
	buf->length += needed;
	buf->buffer->contents[buf->length] = '\0';
}

static inline
void
ps_strfmt_float(ps_strbuf *buf, float f) {
	// Same idea as ps_strfmt_int
	size_t rem = (buf->buffer->length - buf->length) - 1;
	int needed = snprintf(buf->buffer->contents + buf->length, rem, "%f", f);

	if(rem < needed) {
		ps_strbuf_reserve(buf, needed + 1);

		snprintf(buf->buffer->contents + buf->length, rem, "%f", f);
	}

	buf->length += needed;
	buf->buffer->contents[buf->length] = '\0';
}

static inline
void
ps_strfmt_bool(ps_strbuf *buf, ps_bool b) {
	// TODO: Consider using a helper function for this.
	if(b) {
		ps_strbuf_reserve(buf, sizeof("true"));
		memcpy(buf->buffer->contents + buf->length, "true", sizeof("true"));
		buf->length += sizeof("true");
	}
	else {
		ps_strbuf_reserve(buf, sizeof("false"));
		memcpy(buf->buffer->contents + buf->length, "false", sizeof("false"));
		buf->length += sizeof("false");
	}
}

static inline
void
ps_strfmt_str(ps_strbuf *buf, const ps_str *str) {
	// TODO: Do we need the +1 here for the nul terminator?
	ps_strbuf_reserve(buf, str->length + 1);

	// Copy the string and NUL terminator
	memcpy(buf->buffer->contents + buf->length, str->contents, str->length + 1);

	buf->length += str->length;
}

// We need a separate ps_strfmt method for strbuf, because the strbuf has
// a separate length from its internal str.
static inline
void
ps_strfmt_strbuf(ps_strbuf *buf, const ps_strbuf *other) {
	// TODO: Do we need the +1 here for the nul terminator?
	ps_strbuf_reserve(buf, other->length + 1);

	// Copy the string and NUL terminator
	memcpy(buf->buffer->contents + buf->length, other->buffer->contents, other->length + 1);

	buf->length += other->length;
}

// Promotes a Str or a StrConst to a StrBuf by allocating a new buffer and copying
// the characters over.
static inline
ps_strbuf*
ps_promote_str_to_buf(const ps_str* input) {
	// TODO: Maybe consider rounding to nearest power-of-two somewhere?

	// Add 1 so that we have room for the NUL terminator.
	ps_strbuf *buf = ps_strbuf_new(input->length + 1);
	buf->length = input->length;

	memcpy(buf->buffer->contents, input->contents, input->length + 1);

	return buf;
}

static inline
ps_str*
ps_promote_str_const_to_str(const ps_str* input) {
	return ps_str_from_literal_size(input->contents, input->length);
}

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
ps_print_bool(ps_bool b) {
	if(b) { printf("true"); } else { printf("false"); }
}

// Note: functions are like, struct { fun; closure; }

static inline
void
ps_print_ptr(const char *tag, uintptr_t ptr) {
	printf("<%s %" PRIxPTR ">", tag, ptr);
}

// NOTE: We can currently use ps_print_str for StrBufs as well. This is
// because right now ps_print_str does not use the length value.
//
// If we do eventually use the length value, we will have to add a
// ps_print_strbuf() method, as the length value will be different from its
// internal str.
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

static inline
float
ps_promote_int_to_float(ps_int v) { return (ps_float)v; }

#endif