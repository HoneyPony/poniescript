# PonieScript
PonieScript is the scripting language for [PonyGame](https://github.com/HoneyPony/ponygame). It is statically typed, AOT compiled (to C), and designed to be usable for game scripting.

The original philosophy of PonieScript is that C itself--with some well tuned macros and APIs--is "almost good enough" for game scripting, but not quite. PonieScript is therefore intended to be semantically dependent on C,
at least for the foreseeable future--that is, it is philosophically a C preprocessor that is just "a little complicated." PonieScript has diverged a bit from this philosophy by now but it is still somewhat relevant.

Although PonieScript is semantically dependent on C, a core goal is to make the semantics of PonieScript quite precise. That is,
correctness (of the compiler implementation) is more important than speed (of the compiled code).

## WORK IN PROGRESS

PonieScript is currently a heavy work in progress. There are many compiler bugs and limitations at this time. It is slowly converging on a working product.

## Build process

Right now, PonieScript cannot actually be used with PonyGame. It is currently heavily in development. It can still be used standalone, although not very usefully at the moment. The PonieScript compiler is written in Rust so compiling should
be as simple as:

`cargo build`

To use PonieScript requires a C compiler. It currently expects to be run in the working directory of this git repo -- that is, it generates C files that `#include "poni/poni.h"`, where the 'poni' directory is the very same one in this repo.

## Examples

Instead of the traditional `main`, PonieScript allows for an (optional) entry point of `init` -- which corresponds to the initialization code for a PonyGame game. For standalone apps, you can use `init` as the entry point of the entire program.

Hello world is straightforward:

```c
fun init() {
    print("Hello, world!");
}
```

Here is an example showing some function syntax:
```c
fun fib(n: int) -> int {
    if n < 2 { return n; }

    // Implicit return at end of blocks.
    // (Either with or without semicolon).
    fib(n - 1) + fib(n - 2);
}

fun helper(n: int) {
    print("fib(", n, ") = ", fib(n));
}

fun init() {
    helper(5);
    helper(10);
    helper(15);
}
```

Here is an example showing a little bit about classes:
```c
class Pony {
    var greeting = "neigh";

    fun greet() {
        print(greeting);
    }
}

fun init() {
    var pony0 = new Pony{};
    var pony1 = new Pony { greeting: "good afternoon" };

    pony0.greet();
    pony1.greet();
}
```
