#ifndef PONI_STANDALONE_H
#define PONI_STANDALONE_H

// "standalone" refers to poniescripts that are compiled without the addition
// of the ponygame game engine.
//
// For now, it is not clear whether we will support the GC for these applications,
// which makes them somewhat less useful. But, they are still useful for writing
// integration tests for poniescript.

// Standalone programs include their own main() function.

void poni_init_strings(void);
void poni_init_globals(void);
void poni_init(void);

int
main(int argc, char **argv) {
	// Must do strings before globals
	poni_init_strings();
	poni_init_globals();
	poni_init();
}

#endif