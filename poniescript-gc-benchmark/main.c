#include "poni/poni.h"
// --- imports ---

// --- tag definitions ---
#define PONI_TAG_TY10 0x8000000000000100ULL
#define PONI_TAG_TY11 0x8000000000000102ULL
#define PONI_TAG_TY12 0x8000000000000104ULL
#define PONI_TAG_TY13 0x8000000000000106ULL
#define PONI_TAG_TY14 0x8000000000000108ULL
#define PONI_TAG_TY15 0x800000000000010aULL
#define PONI_TAG_TY18 0x10cULL
#define PONI_TAG_TY22 0x10eULL
#define PONI_TAG_TY23 PONI_TAG_TY22
#define PONI_TAG_TY24 0x112ULL
#define PONI_TAG_TY25 0x8000000000000114ULL

// --- string constants ---

// --- struct declarations ---

struct cl_LinkedList;
// --- struct declarations (ps_tuple) ---
struct ps_range_ie_25;

// --- struct declarations (ps_array) ---

// --- sig types ---
typedef struct cl_LinkedList* (*ps_sigraw_2)(struct poni_gc_context*, ps_int, void*);
typedef struct ps_sig_2 { ps_sigraw_2 fun; void* closure; } ps_sig_2;

// --- struct definitions (ps_tuple) ---
struct ps_range_ie_25 {
	ps_int left;
	ps_int right;
};

// --- struct definitions (ps_array) ---

// --- struct definitions ---
struct cl_LinkedList {
	struct ps_object object;
	struct cl_LinkedList* v_next;
};
// --- global variables ---
struct cl_LinkedList* v_big_list;

// --- function declarations ---
struct cl_LinkedList* f_make_list(PONI_ABI(ps_int v_size));
void update(PONI_ABI());
void poni_init_strings(struct poni_gc_context *ctx) {
}

// --- gc support ---
static inline size_t
poni_get_type_stride(uint64_t tag) {
	switch(tag) {
		// Memory safety: Don't let us access an invalid size.
		default:
			abort();
			return 0;
		case PONI_TAG_STRCONST:
		case PONI_TAG_STR:
		case PONI_TAG_STRBUF:
		case PONI_TAG_ARRAY:
		case PONI_TAG_DYNARRAY:
			return sizeof(void*);
		case PONI_TAG_FLOAT: return sizeof(ps_float);
		case PONI_TAG_INT:   return sizeof(ps_int);
		case PONI_TAG_BOOL:  return sizeof(ps_bool);
		case PONI_TAG_TY10: return sizeof(ps_vec2);
		case PONI_TAG_TY11: return sizeof(ps_vec3);
		case PONI_TAG_TY12: return sizeof(ps_vec4);
		case PONI_TAG_TY13: return sizeof(ps_vec2i);
		case PONI_TAG_TY14: return sizeof(ps_vec3i);
		case PONI_TAG_TY15: return sizeof(ps_vec4i);
		case PONI_TAG_TY25: return sizeof(struct ps_range_ie_25);
		case PONI_TAG_TY22:
			return sizeof(void*);
		case PONI_TAG_TY18:
		case PONI_TAG_TY24:
			return sizeof(struct { void (*fn)(void); void *closure; });
	}
}
static inline ps_bool
poni_is_value_type(uint64_t tag) { return !!(tag & 0x8000000000000000ULL); }
void
poni_gc_visit_valuetype(struct poni_gc *gc, void *object, uint64_t tag) {
	switch(tag) {
	case PONI_TAG_TY10: break;
	case PONI_TAG_TY11: break;
	case PONI_TAG_TY12: break;
	case PONI_TAG_TY13: break;
	case PONI_TAG_TY14: break;
	case PONI_TAG_TY15: break;
	case PONI_TAG_TY24: {
		ps_sig_2 *self = object;
		poni_gc_mark(gc, self->closure);
		break;
	}
	}
}
void
poni_gc_visit_object(struct poni_gc *gc, void *object) {
    uint64_t tag = *(uint64_t*)object;
    switch(tag & 0xFFFFFFFFFFFFFFFEULL) {
		case PONI_TAG_STRCONST:
		case PONI_TAG_STR:
			break; // Nothing to do
		case PONI_TAG_STRBUF: {
			struct ps_strbuf *self = object;
			poni_gc_mark(gc, self->buffer);
			break;
		}
		case PONI_TAG_ARRAY: {
			struct ps_array_header *header = object;
			char *elem_root = (char*)object + sizeof(struct ps_array_header);
			if(poni_is_value_type(header->type)) {
				// As an optimization, never visit any objects inside an
				// array of primitive types. We should probably have an additional
				// type info function that tells us whether we need to iterate
				// here.
				if(header->type == PONI_TAG_INT || header->type == PONI_TAG_FLOAT
					|| header->type == PONI_TAG_BOOL)
				{ break; }

				size_t stride = poni_get_type_stride(header->type);

				// For value types, the inner objects do not themselves need
				// to be marked; so instead of going through the gc marker,
				// instead just visit them directly.
				for(ps_int i = 0; i < header->length; ++i) {
					poni_gc_visit_valuetype(gc, elem_root, header->type);
					elem_root += stride;
				}
			}
			else {
				size_t stride = poni_get_type_stride(header->type);

				for(ps_int i = 0; i < header->length; ++i) {
					uintptr_t as_ptr = *(uintptr_t*)(elem_root);
					poni_gc_mark(gc, (void*)as_ptr);
					elem_root += stride;
				}
			}
			break;
		}
		case PONI_TAG_DYNARRAY: {
			struct ps_dynarray_header *header = object;
			// For memory safety reasons, we need to be walking a known-good
			// pointer. So, store the pointer ahead of time, as its size
			// is immutable.
			//
			// We also cannot directly mark the inner buffer. The problem is
			// that it may have undefined contents, outside of the boundaries
			// of this array. So, instead we directly mark the inner buffer,
			// and then walk the children manually.
			struct ps_array_header *inner = header->buffer;
			poni_gc_mark(gc, inner);
			char *elem_root = (char*)inner + sizeof(struct ps_array_header);

			// Note that although the DynArray's length might be changed by
			// another thread, we shouldn't have any code that can invalidate
			// existing objects in the array (aside from maybe this code).
			//
			// So even if the length decreases after we read it, we shouldn't
			// end up reading an invalid object.
			//
			// (*writes* to the length will perhaps have to be atomic-acquire?
			// they *must* occur *after* any writes to the buffer contents).
			ps_int length = header->length;
			if(inner->length < length) { length = inner->length; }

			// Now the rest of the logic is essentially the same as the regular
			// arrays.
			if(poni_is_value_type(header->type)) {
				if(header->type == PONI_TAG_INT || header->type == PONI_TAG_FLOAT
					|| header->type == PONI_TAG_BOOL)
				{ break; }

				size_t stride = poni_get_type_stride(header->type);

				for(ps_int i = 0; i < length; ++i) {
					poni_gc_visit_valuetype(gc, elem_root, header->type);
					elem_root += stride;
				}
			}
			else {
				size_t stride = poni_get_type_stride(header->type);

				for(ps_int i = 0; i < length; ++i) {
					uintptr_t as_ptr = *(uintptr_t*)(elem_root);
					poni_gc_mark(gc, (void*)as_ptr);
					elem_root += stride;
				}
			}

			break;
		}
	case PONI_TAG_TY22: {
		struct cl_LinkedList *self = object;
		poni_gc_mark(gc, self->v_next);
		break;
	}
	}
}
void
poni_gc_visit_roots(struct poni_gc *gc) {
	poni_gc_mark(gc, v_big_list);
}
size_t
poni_gc_get_allocation_size(void *object) {
	uint64_t tag = *(uint64_t*)object;
	switch(tag) {
		// Memory safety: Don't let us access an invalid size.
		default:
			abort();
			return 0;
		case PONI_TAG_STRCONST:
		case PONI_TAG_STR:
		{
			struct ps_str *self = object;
			return sizeof(*self) + self->length;
		}
		case PONI_TAG_STRBUF: {
			return sizeof(struct ps_strbuf);
		}
		case PONI_TAG_ARRAY: {
			struct ps_array_header *header = object;
			size_t stride = poni_get_type_stride(header->type);
			return sizeof(*header) + stride * header->length;
		}
		case PONI_TAG_DYNARRAY: {
			return sizeof(struct ps_dynarray_header);
		}
	case PONI_TAG_INT: return sizeof(ps_int);
	case PONI_TAG_FLOAT: return sizeof(ps_float);
	case PONI_TAG_BOOL: return sizeof(ps_bool);
	case PONI_TAG_TY10: return sizeof(ps_vec2);
	case PONI_TAG_TY11: return sizeof(ps_vec3);
	case PONI_TAG_TY12: return sizeof(ps_vec4);
	case PONI_TAG_TY13: return sizeof(ps_vec2i);
	case PONI_TAG_TY14: return sizeof(ps_vec3i);
	case PONI_TAG_TY15: return sizeof(ps_vec4i);
	case PONI_TAG_TY18: return sizeof(struct { void *a, *b; });
	case PONI_TAG_TY22: return sizeof(*(struct cl_LinkedList*)(0));
	case PONI_TAG_TY24: return sizeof(struct { void *a, *b; });
	case PONI_TAG_TY25: return sizeof(struct ps_range_ie_25);
	}
}

void poni_init_globals(struct poni_gc_context *ctx) {
	struct cl_LinkedList* t0 = f_make_list(ctx, ((ps_int)50000000), NULL);
	v_big_list = t0;

}
void poni_init(struct poni_gc_context *ctx) {}
// --- function definitions ---
void update(PONI_ABI()) {
	PONI_GC_FRAME(1, "update");
	{
		struct cl_LinkedList* t0 = f_make_list(ctx, ((ps_int)10000), NULL);
	}
	ctx->frame = gc_frame.prev;
}
struct cl_LinkedList* f_make_list(PONI_ABI(ps_int v_size)) {
	PONI_GC_FRAME(5, "make_list");
	struct cl_LinkedList* t0;
	{
		struct cl_LinkedList* t1 = poni_gc_alloc_tagged(ctx, sizeof(struct cl_LinkedList), PONI_TAG_TY22);
		t1->v_next = NULL;
		struct cl_LinkedList* v_list = t1;
		struct cl_LinkedList* v_head = v_list;
		{
			struct ps_range_ie_25 t2;
			t2.left = ((ps_int)0);
			t2.right = v_size;
			ps_int t3 = t2.left;
			ps_int v0_i = (t3 - ((ps_int)1));
			for(;;) {
				struct ps_range_ie_25 t4;
				t4.left = ((ps_int)0);
				t4.right = v_size;
				ps_int t5 = t4.right;
				if(!(ps_bool)(v0_i < (t5 - ((ps_int)1)))) { break; }
				{
					v0_i = (v0_i + ((ps_int)1));
					ps_int v_i = v0_i;
					{
						struct cl_LinkedList* t6 = poni_gc_alloc_tagged(ctx, sizeof(struct cl_LinkedList), PONI_TAG_TY22);
						t6->v_next = NULL;
						struct cl_LinkedList* v0_next = t6;
						struct cl_LinkedList* t7;
						t7 = (v0_next);
						struct cl_LinkedList* t8 = v_head->v_next = t7;
						v_head = v0_next;
					}
				}
			}
		}
		t0 = v_list;
	}
	ctx->frame = gc_frame.prev;
	return t0;
}
