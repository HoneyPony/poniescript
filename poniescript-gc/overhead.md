## Sources of overhead in the PonieScript garbage collector

Ideally we keep the overhead of the garbage collector somewhat low. Of course,
there is going to be some unavoidable overhead.

1. **ABI Requirements**

   Every function that might need to interact with the GC must have a poni_gc_context*
argument. This uses up one of the register slots for function arguments, and also
requires the compiler to shuffle the context pointer into and out of the relevant
argument as needed.

   This could be mitigated a couple of ways:
   - We could reserve a register for storing the context pointer, and then use
     compiler options to tell the compiler not to use that register. This would
     cut down on the amount of register shuffling, but would instead result in
     completely removing one register from the equation, which could be worse.
   - Leaf functions that don't need to interact with the GC could be marked as
     such and use a different ABI.

2. **Safepoints**

   So that the garbage collector is able to make progress, it must be able to
   preempt each thread at regular intervals. This requires safepoints to be compiled
   into the code pervasively.

   However, this is actually one of the easier ones to mitigate. For game engine
   application specifically, we can simply turn off safepoint compilation into the
   main body of the code, and insert a single safepoint into the main game loop.
   This is enough to make progress in the garbage collector (updates at e.g.
   60hz) and completely avoids any overhead from safepoints in the vast majority
   of the code.

3. **Write barriers**

   Write barriers are, it seems, unavoidable for the basic construction of a
   concurrent GC. It simply must account for the possibility that a pointer to
   some living object is only available through a previously-visited object, and
   so the only way for the GC to correctly mark that living object is either to
   revisit the previously-visited object, or to have a write barrier.

   However, write barriers may not be as bad as they seem. Yes, they must be
   pervasive in the code. However, writes do not actually occur that often,
   especially in game scripting code.

4. **Atomic pointer writes**

   It is critical that any time we write to a pointer, the write is atomic:
   if we ever read a partially-written pointer inside the main GC loop, very bad
   things would happen.

   However, this does not appear to actually be a problem. For most of the platforms
   we care about, it seems that a "relaxed" memory order write to a pointer is
   identical to just writing to it. The only one that's a little bit unclear
   is WebAssembly; however, it should be the case that its atomic writes are
   also implemented as the relevant equivalent instructions when run.