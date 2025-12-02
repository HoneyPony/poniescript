#ifndef PONI_STANDALONE_H
#define PONI_STANDALONE_H

#include "poni.h"
#include "poni_gc.h"

// "standalone" refers to poniescripts that are compiled without the addition
// of the ponygame game engine.
//
// For now, it is not clear whether we will support the GC for these applications,
// which makes them somewhat less useful. But, they are still useful for writing
// integration tests for poniescript.

// Standalone programs include their own main() function.

void poni_init_strings(struct poni_gc_context *ctx);
void poni_init_globals(struct poni_gc_context *ctx);
void poni_init(struct poni_gc_context *ctx);

int
main(int argc, char **argv) {
	struct poni_gc_handle *gc_handle = poni_gc_spawn();
	struct poni_gc_context *ctx = poni_gc_create_context_for_existing(gc_handle);

	// Must do strings before globals
	poni_init_strings(ctx);
	poni_init_globals(ctx);
	poni_init(ctx);

#ifdef PONI_CLEAN_EXIT
	// For testing purposes, we would like to:
	// 1) Trigger a GC
	// 2) Wait for everything to be collected
	//
	// This should make sure that GC integration at least basically works.
	//
	// This can be done by defining the PONI_CLEAN_EXIT define, above.
	poni_gc_send_request(gc_handle, PONI_GC_REQUEST_COLLECT);
	poni_gc_join(gc_handle, ctx);

	poni_gc_free_context(ctx);
	poni_gc_free_handle(gc_handle);
#else
	// For practical purposes, there is no reason to free everything in the GC.
	// It is literally a waste of time. So, instead just exit.
	return 0;
#endif
}

#endif