#ifndef IMPORT_FUN_H
#define IMPORT_FUN_H

#include "poni/poni_glue.h"
#include "poni/poni.h"

static inline
PS_FUN()
ps_int
add(ps_int x, ps_int y) {
    return x + y;
}

static inline
PS_FUN("sub")
ps_int
subtract(ps_int x, ps_int y) {
    return x - y;
}

#endif