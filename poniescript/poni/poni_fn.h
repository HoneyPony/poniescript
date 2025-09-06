#ifndef PONI_FN_H
#define PONI_FN_H

#include <stdint.h>

struct ps_fun_ref {
    uintptr_t fn;
    uintptr_t closure;
};

#define PS_CALL(fun_ref, ...) do { \
    if(fun_ref.fn & 1ULL) { \
        if(fun_ref.closure) {
            PS_CALL_
        }
    } \
} while(0)

#endif