all: bin-tcc bin-clang bin-gcc

bin-tcc: host.o caller-tcc.o
	clang $^ -o $@
bin-gcc: host.o caller-gcc.o
	clang $^ -o $@
bin-clang: host.o caller-clang.o
	clang $^ -o $@

# Host is compiled by clang
host.o: host.c lib.h
	clang -c $< -o $@

# Compare compilation of caller by tcc and clang
caller-clang.o: caller.c lib.h
	clang -c $< -o $@
caller-gcc.o: caller.c lib.h
	gcc -c $< -o $@
caller-tcc.o: caller.c lib.h
	tcc -c $< -o $@

.PHONY: all