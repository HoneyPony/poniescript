#ifndef IMPORT_CLASS_H
#define IMPORT_CLASS_H

#include "poni/poni_glue.h"
#include "poni/poni_abi.h"
#include "poni/poni.h"

// We don't know for sure how the tag system will work yet. I think it will
// have to be something like:
// - The compiler collects dyntag's for external modules. These are used when
//   the module is pre-compiled, while the non-dynamic tag TAG_TEST can be
//   re-defined as a static value when the code is compiled by the compiler.
static uint64_t dyntag_Test = 100;

#define TAG_TEST dyntag_Test

PS_CLASS("Test")
struct test {
    struct ps_object object;

    // TODO: It is OK for the user to manually construct a C-defined class
    // if ALL the state it needs is C-exported (or, I suppose, if we have
    // some sort of __post_init function).
    //
    // In any case, the point is that we need a way to mark classes as OK-to-construct
    // directly in PonieScript code, and a way to NOT mark them as that.
    PS_VAR() ps_int value;
};

// In order to actually construct a test, we more or less need a function.

// static inline
// PS_FUN()
// struct test*
// get_a_test(PS_ABI(ps_int value)) {
//     // TODO: How are we going to assign tags to each module??
//     struct test* new_test = ps_gc_must_calloc(sizeof(*new_test), TAG_TEST);
//     new_test->value = value;
//     return new_test;
// }

#endif