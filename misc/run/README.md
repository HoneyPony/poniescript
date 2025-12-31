# poni_run: Sketch of hot-code reloading for PonieScript

To use this, cd into the root of the repo (because PonieScript requires the
header files for now).

Then, run:

./misc/run/poni_run

And, in another window, run something like:

poniescript -e misc/run/poni_run.poni -o game-a.so

Then, make some tweaks to poni_run.poni, and run:

poniescript -e misc/run/poni_run.poni -o game-b.so

Then go back to `game-a.so`, then `game-b.so`, and so forth.

Note that global variables do NOT keep their value between runs. This will
have to be something we think about for PonieScript. (Maybe globals can be
registered in some kind of thing that looks them up from the hot code host,
at least for reloading?)

## Sketch of global variables

`var a := 0;`

```c
ps_int *hot_a = NULL;

void
poni_lookup_globals() {
    // Look up variables based on their C declaration. That way, incompatible
    // types will return different variables.
    //
    // Of course, we may have to do something to translate certain kinds of
    // variables, especially classes that have members added/removed. That was
    // sort of the copying garbage collection idea.
    bool init;
    hot_a = poni_host_lookup("ps_int *hot_a", 4, &init);

    // 'init' is set to true if the variable did not already have a value that
    // we are trying to retain.
    if(init) {
        hot_a = 0;
    }
}
```

Likely we will want to have a special hot-code reloading mode in the compiler
that does a few things.

First, it will generate any special setup for globals and similar.

But second, it also needs to generate some sort of "comparison database" that
it can load. This will be important for doing things like changing the type of
something in-between reloads. 