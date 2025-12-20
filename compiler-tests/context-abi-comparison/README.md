This comparison is between two different versions of the 'context' API.

In particular, in the concurrent garbage collector world, each thread needs
its own &GcContext (or more straightforwardly, a *ctx pointer of some sort).

There are two possible ways to implement this:
1) (The original way): pass the context pointer to every function in the chain.
2) Have the context pointer be a thread-local.

The problem with (1) is that it is overhead in every single function call--
every function calls that don't allocate--as well as more register/memory pressure
(e.g. the compiler may have to keep the *ctx pointer in a register, or othewise
save it to stack, whereas with a thread local it would not have to).

The problem with (2) is that thread locals are not necessarily fast. But,
on the other hand, not every single function allocates. Also, simplifying the
ABI would be nice for writing bindings, especially in Rust (?).

This is a bit of a microbenchmark to see which one might be better.
