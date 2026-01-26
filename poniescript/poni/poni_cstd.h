#ifndef PONI_CSTD_H
#define PONI_CSTD_H

#ifdef PONI_WASM32

// We assume a clang target for now.
typedef __UINT64_TYPE__ uint64_t;
typedef __INT64_TYPE__  int64_t;

typedef __INT8_TYPE__ int8_t;
typedef __INT32_TYPE__ int32_t;

typedef __SIZE_TYPE__ size_t;
typedef __UINTPTR_TYPE__ uintptr_t;

#define PRId64 "%ld"
#define PRIxPTR "%p"

#define NULL ((void*)0)

// The callers of these need to be ported to rust-rt, I think...
#define printf(...)
#define snprintf(...) ((size_t)0)

inline void
exit(int code) {
    volatile int i = 0;
    // Infinite loop
    while(i == 0) {}
}

#define abort() exit(-1)

inline void*
memcpy(void *restrict dst, const void *restrict src, size_t n)
{
    char       *dst_char =       (char*)dst;
    const char *src_char = (const char*)src;

    for (size_t i = 0; i < n; ++i) {
        dst_char[i] = src_char[i];
    }

    return dst;
}

inline size_t
strlen(const char *str) {
    size_t len = 0;
    while(str[len]) { len += 1; }
    return len;
}

#else

#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <inttypes.h> // for ps_panic

#if defined(__STDC_NO_ATOMICS__) || defined(__TINYC__)
#else
    #include <stdatomic.h>
#endif

#endif

#endif