/*
 * This file is meant to test whether a static function with a parameter that
 * is unused can be optimized by the compiler.
 * 
 * Specifically, we would like to do something like:
 *
 *    void fn_signature(void *object, ...args)
 *
 * For all of our functions--this makes closures very naturally "just classes"
 * while still letting us pass other functions as function pointers.
 *
 * BUT! In this case, we are wasting time loading stuff into the first parameter,
 * which is silly. Ideally, the compiler simply doesn't actually bother loading
 * anything into that parameter, if the parameter is really unused.
 *
 * If a function is static, it can only be used in this translation unit,
 * so if the compiler is ever going to do this optimization--it will be here.
 */

#include <stdio.h>

__attribute__((noinline))
void
public_noattr(void *firstarg, int a, int b) {
    printf("%d\n", a + b);
}

__attribute__((noinline))
void
public_attr(__attribute__((unused)) void *firstarg, int a, int b) {
    printf("%d\n", a + b);
}

__attribute__((noinline))
static void
static_noattr(void *firstarg, int a, int b) {
    printf("%d\n", a + b);
}

__attribute__((noinline))
static void
static_attr(__attribute__((unused)) void *firstarg, int a, int b) {
    printf("%d\n", a + b);
}

int
main() {
    public_noattr((void*)1, 10, 20);
    public_attr((void*)2, 11, 21);
    static_noattr((void*)3, 13, 23);
    static_attr((void*)4, 18, 28);
}