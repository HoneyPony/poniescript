main: main.o ../target/release/libponiescript_gc_benchmark.a
	gcc $^ -o $@

main.o: main.c
	gcc -c $^ -o $@ -O2 -I../poniescript -g

main.c: main.poni
	poniescript $^ -e --bind-fun update -o $@

../target/release/libponiescript_gc_benchmark.a:
	cargo build --release