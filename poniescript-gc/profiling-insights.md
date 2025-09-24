## Sept 22, 2025

Here I am investigating the following code:

```c
struct object2*
myfun(struct poni_gc_context *ctx) {
    PONI_FRAME(3, struct object1 *obj1_1; struct object1 *obj1_2; struct object2 *obj2;)

    frame.obj1_1 = mk_obj1(ctx, 10, 20);
    frame.obj1_2 = mk_obj1(ctx, 30, 40);
    frame.obj2 = mk_obj2(ctx, frame.obj1_1, frame.obj1_2);

    PONI_RETURN(frame.obj2);
}

int
main(int argc, char **argv) {
    struct poni_gc_handle *handle = poni_gc_spawn();
    struct poni_gc_context *ctx = poni_gc_create_context_for_existing(handle);

    // Also, add code to the GC to repeatedly collect.
    poni_gc_send_request(handle, PONI_GC_REQUEST_COLLECT);

    PONI_FRAME(1, struct object2 *myobj;)
    for(int i = 0; i < 100000; ++i) {
        frame.myobj = myfun(ctx);

        // PONI_GC_SAFEPOINT(ctx);
    }
}
```

The question is, why does uncommenting PONI_GC_SAFEPOINT in this loop lead
to almost a 4x slowdown, whether with the default allocator or with mimalloc?

According to the profiler (samply), most of the time spent in the slow case is not,
in fact, inside PONI_GC_SAFEPOINT, but rather inside poni_gc_alloc. What?

The profiler gives insight though: It tells us a lot of time is spent inside the
mutex contended case.

I knew that the mutex in the allocator would definitely cause issues, and slowdowns,
especially in the sweep case. What I didn't think about was exactly how bad it
would be.

This, to my understanding, is basically what is happening:
1. When we add PONI_GC_SAFEPOINT, it allows the collector to make progress, to
   the point where it is able to sweep.
2. Once the collector reaches the sweeping point, now we suddenly must synchronize
   with the end of the collect cycle in the main loop.
3. But the sweep is going to take forever, as it has to walk through a huge vector
   and free everything.

So essentially, I accidentally made the main loop wait for every collect cycle
to finish (and in particular finish sweeping), but what we'd really like is for
allocation to just continue unimpeded during the sweeping.

I knew that I wanted to get rid of the "single mutex for allocation" anyway,
but this gives a completely different reason why -- it's not just contended allocation
that's a problem, but really contended sweeping (although I definitely had thought
of that before).

## To copy or to move?

It's not really clear what the best way to handoff the vector of allocations is.

Copying it seems to be faster, but moving it seems to be faster in the case that
we very, very occasionally safepoint. I suppose that makes some sense -- rarer
safepoints result in more to copy -- but the crossover seems somewhat surprising.

## More insights on the loop test

Reducing the number of GC's to just four inside the loop -- just four -- takes
almost exactly the same amount of time as doing a GC as often as possible.

I think this shows that, by far, the majority of the overhead from the GC is
simply the fact that mimalloc has to dealloc at the same time as it is allocing.

(it is, however, the case that doing only one gc inside the loop seems to be 
quite a bit faster).

I'm not sure the safepointing itself can really get any faster, then. Almost
all of the overhead is just going to be allocator interactions.