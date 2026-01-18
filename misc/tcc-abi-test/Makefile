all: bin-tcc bin-clang

bin-tcc: host.o caller-tcc.o
	clang -fuse-ld=mold $^ -o $@
bin-clang: host.o caller-clang.o
	clang -fuse-ld=mold $^ -o $@

# Host is compiled by clang
host.o: host.c
	clang -c $^ -o $@ -I../../poniescript

# Compare compilation of caller by tcc and clang
caller-clang.o: caller.c
	clang -c $^ -o $@ -I../../poniescript
caller-tcc.o: caller.c
	tcc -c $^ -o $@ -I../../poniescript

.PHONY: all