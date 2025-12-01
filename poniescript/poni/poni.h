#ifndef PONI_H
#define PONI_H

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <inttypes.h>

#include "poni_gc.h"

#define PONI_TAG_FLOAT    0x8000000000000002ULL
#define PONI_TAG_INT      0x8000000000000004ULL
#define PONI_TAG_BOOL     0x8000000000000006ULL
#define PONI_TAG_STRCONST 8
#define PONI_TAG_STR      10
#define PONI_TAG_STRBUF   12
#define PONI_TAG_ARRAY    14

typedef float   ps_float;
typedef int32_t ps_int;
typedef int8_t  ps_bool;

struct ps_object;
struct ps_str;
struct ps_strbuf;

typedef struct ps_vec2 {
	union {
		struct {
			ps_float x;
			ps_float y;
		};
		struct {
			ps_float v_0;
			ps_float v_1;
		};
		ps_float at[2];
	};
} ps_vec2;

static inline
ps_vec2
ps_lerp_vec2(ps_vec2 a, ps_vec2 b, ps_float t) {
	ps_float s = 1.0 - t; 
	return (ps_vec2){
		.x = s * a.x + t * b.x,
		.y = s * a.y + t * b.y,
	};
}

typedef struct ps_vec3 {
	union {
		struct {
			ps_float x;
			ps_float y;
			ps_float z;
		};
		struct {
			ps_float v_0;
			ps_float v_1;
			ps_float v_2;
		};
		ps_float at[3];
	};
} ps_vec3;

static inline
ps_vec3
ps_lerp_vec3(ps_vec3 a, ps_vec3 b, ps_float t) {
	ps_float s = 1.0 - t; 
	return (ps_vec3){
		.x = s * a.x + t * b.x,
		.y = s * a.y + t * b.y,
		.z = s * a.z + t * b.z,
	};
}

typedef struct ps_vec4 {
	union {
		struct {
			ps_float x;
			ps_float y;
			ps_float z;
			ps_float w;
		};
		struct {
			ps_float v_0;
			ps_float v_1;
			ps_float v_2;
			ps_float v_3;
		};
		ps_float at[4];
	};
} ps_vec4;

static inline
ps_vec4
ps_lerp_vec4(ps_vec4 a, ps_vec4 b, ps_float t) {
	ps_float s = 1.0 - t; 
	return (ps_vec4){
		.x = s * a.x + t * b.x,
		.y = s * a.y + t * b.y,
		.z = s * a.z + t * b.z,
		.w = s * a.w + t * b.w,
	};
}

typedef struct ps_vec2i {
	union {
		struct {
			ps_int x;
			ps_int y;
		};
		struct {
			ps_int v_0;
			ps_int v_1;
		};
		ps_int at[2];
	};
} ps_vec2i;

static inline
ps_vec2i
ps_lerp_vec2i(ps_vec2i a, ps_vec2i b, ps_float t) {
	ps_float s = 1.0 - t; 
	return (ps_vec2i){
		.x = (ps_int)(s * a.x + t * b.x),
		.y = (ps_int)(s * a.y + t * b.y),
	};
}

typedef struct ps_vec3i {
	union {
		struct {
			ps_int x;
			ps_int y;
			ps_int z;
		};
		struct {
			ps_int v_0;
			ps_int v_1;
			ps_int v_2;
		};
		ps_int at[3];
	};
} ps_vec3i;

static inline
ps_vec3i
ps_lerp_vec3i(ps_vec3i a, ps_vec3i b, ps_float t) {
	ps_float s = 1.0 - t; 
	return (ps_vec3i){
		.x = (ps_int)(s * a.x + t * b.x),
		.y = (ps_int)(s * a.y + t * b.y),
		.z = (ps_int)(s * a.z + t * b.z),
	};
}

typedef struct ps_vec4i {
	union {
		struct {
			ps_int x;
			ps_int y;
			ps_int z;
			ps_int w;
		};
		struct {
			ps_int v_0;
			ps_int v_1;
			ps_int v_2;
			ps_int v_3;
		};
		ps_int at[4];
	};
} ps_vec4i;

static inline
ps_vec4i
ps_lerp_vec4i(ps_vec4i a, ps_vec4i b, ps_float t) {
	ps_float s = 1.0 - t; 
	return (ps_vec4i){
		.x = (ps_int)(s * a.x + t * b.x),
		.y = (ps_int)(s * a.y + t * b.y),
		.z = (ps_int)(s * a.z + t * b.z),
		.w = (ps_int)(s * a.w + t * b.w),
	};
}

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

struct ps_array_header {
	/** Object header */
	ps_object object;
	/** Type of the array members */
	uint64_t  type;
	/** Length of the array */
	ps_int    length;
};

#ifdef __TINYC__
	#define PONI_NORETURN __attribute__((noreturn))
#else
	#define PONI_NORETURN _Noreturn
#endif

#define PONI_INIT_ARRAY(arr, elem_sz, elem_cnt, elem_tag) \
	arr = poni_gc_alloc_tagged(ctx, sizeof(struct ps_array_header) + elem_sz * elem_cnt, PONI_TAG_ARRAY); \
	arr->header.length = elem_cnt; \
	arr->header.type   = elem_tag;

static inline
PONI_NORETURN void
ps_fatal_error(const char *message) {
	printf("fatal error: %s\n", message);
	exit(1);
}

static inline
ps_str*
ps_str_from_literal_size(struct poni_gc_context *ctx, const char *input, size_t length) {
	size_t bytes = sizeof(ps_str) + ((length + 1) * sizeof(char));
	ps_str *str =  poni_gc_alloc_tagged(ctx, bytes, PONI_TAG_STRCONST);

	memcpy(str->contents, input, length);
	str->contents[length] = '\0';
	str->length = length;

	return str;
}

static inline
ps_str*
ps_str_from_alloc(struct poni_gc_context *ctx, size_t length) {
	// For from_alloc, do not add 1 to length.
	size_t bytes = sizeof(ps_str) + ((length) * sizeof(char));
	ps_str *str =  poni_gc_alloc_tagged(ctx, bytes, PONI_TAG_STRCONST);

	str->length = length;
	return str;
}

// Use sizeof(lit) - 1 because the length value does not include NUL terminator
#define ps_str_from_literal(ctx, lit) ps_str_from_literal_size(ctx, lit, (sizeof(lit) - 1))

static inline
ps_strbuf*
ps_strbuf_new(struct poni_gc_context *ctx, size_t prealloc) {
	ps_strbuf *result =  poni_gc_alloc_tagged(ctx, sizeof(*result), PONI_TAG_STRBUF);
	result->buffer = ps_str_from_alloc(ctx, prealloc);
	result->length = 0;

	return result;
}

static inline
void
ps_strbuf_reserve(struct poni_gc_context *ctx, ps_strbuf *buf, size_t needed) {
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

	buf->buffer = poni_gc_realloc(ctx, buf->buffer, bytes);
	buf->buffer->length = new_len;
}

static inline
void
ps_strfmt_int(struct poni_gc_context *ctx, ps_strbuf *buf, ps_int i) {
	// We will compare the snprintf() result against the total chars -1,
	// because snprintf() returns the length of everything BUT the NUL
	// terminator.
	size_t rem = (buf->buffer->length - buf->length) - 1;
	int needed = snprintf(buf->buffer->contents + buf->length, rem, "%d", i);

	if(rem < needed) {
		// If we didn't have enough room, we will reallocate and do the
		// snprintf() again. Reserve needed + 1 so that we include the NUL terminator.
		ps_strbuf_reserve(ctx, buf, needed + 1);

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
ps_strfmt_float(struct poni_gc_context *ctx, ps_strbuf *buf, float f) {
	// Same idea as ps_strfmt_int
	size_t rem = (buf->buffer->length - buf->length) - 1;
	int needed = snprintf(buf->buffer->contents + buf->length, rem, "%f", f);

	if(rem < needed) {
		ps_strbuf_reserve(ctx, buf, needed + 1);

		snprintf(buf->buffer->contents + buf->length, rem, "%f", f);
	}

	buf->length += needed;
	buf->buffer->contents[buf->length] = '\0';
}

static inline
void
ps_strfmt_bool(struct poni_gc_context *ctx, ps_strbuf *buf, ps_bool b) {
	// TODO: Consider using a helper function for this.
	if(b) {
		ps_strbuf_reserve(ctx, buf, sizeof("true"));
		memcpy(buf->buffer->contents + buf->length, "true", sizeof("true"));
		buf->length += sizeof("true");
	}
	else {
		ps_strbuf_reserve(ctx, buf, sizeof("false"));
		memcpy(buf->buffer->contents + buf->length, "false", sizeof("false"));
		buf->length += sizeof("false");
	}
}

static inline
void
ps_strfmt_str(struct poni_gc_context *ctx, ps_strbuf *buf, const ps_str *str) {
	// TODO: Do we need the +1 here for the nul terminator?
	ps_strbuf_reserve(ctx, buf, str->length + 1);

	// Copy the string and NUL terminator
	memcpy(buf->buffer->contents + buf->length, str->contents, str->length + 1);

	buf->length += str->length;
}

// We need a separate ps_strfmt method for strbuf, because the strbuf has
// a separate length from its internal str.
static inline
void
ps_strfmt_strbuf(struct poni_gc_context *ctx, ps_strbuf *buf, const ps_strbuf *other) {
	// TODO: Do we need the +1 here for the nul terminator?
	ps_strbuf_reserve(ctx, buf, other->length + 1);

	// Copy the string and NUL terminator
	memcpy(buf->buffer->contents + buf->length, other->buffer->contents, other->length + 1);

	buf->length += other->length;
}

// Promotes a Str or a StrConst to a StrBuf by allocating a new buffer and copying
// the characters over.
static inline
ps_strbuf*
ps_promote_str_to_buf(struct poni_gc_context *ctx, const ps_str* input) {
	// TODO: Maybe consider rounding to nearest power-of-two somewhere?

	// Add 1 so that we have room for the NUL terminator.
	ps_strbuf *buf = ps_strbuf_new(ctx, input->length + 1);
	buf->length = input->length;

	memcpy(buf->buffer->contents, input->contents, input->length + 1);

	return buf;
}

static inline
ps_str*
ps_promote_str_const_to_str(struct poni_gc_context *ctx, const ps_str* input) {
	return ps_str_from_literal_size(ctx, input->contents, input->length);
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

static inline void ps_print_vec2(ps_vec2 v) { printf("(%f, %f)", v.x, v.y); }
static inline void ps_print_vec3(ps_vec3 v) { printf("(%f, %f, %f)", v.x, v.y, v.z); }
static inline void ps_print_vec4(ps_vec4 v) { printf("(%f, %f, %f, %f)", v.x, v.y, v.z, v.w); }

static inline void ps_print_vec2i(ps_vec2i v) { printf("(%d, %d)", v.x, v.y); }
static inline void ps_print_vec3i(ps_vec3i v) { printf("(%d, %d, %d)", v.x, v.y, v.z); }
static inline void ps_print_vec4i(ps_vec4i v) { printf("(%d, %d, %d, %d)", v.x, v.y, v.z, v.w); }

static inline
void
ps_print_const(const char *what) {
	printf("%s", what);
}

static inline
void
ps_println(void) {
	putc('\n', stdout);
}

static inline
float
ps_promote_int_to_float(ps_int v) { return (ps_float)v; }

#define PONI_GC_FRAME(in_ptr_count) \
struct { \
	struct poni_gc_frame *prev; \
	uint64_t ptr_count; \
	void *ptrs[in_ptr_count]; \
} gc_frame = {0}; \
gc_frame.ptr_count = in_ptr_count; \
gc_frame.prev = ctx->frame; \
ctx->frame = (void*)&gc_frame

#endif