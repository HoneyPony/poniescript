RUSTLIB=target/release/libponi_gc_rs.a

testgc: test/main.o $(RUSTLIB)
	gcc -g -O2 test/main.o $(RUSTLIB) -o $@

test/main.o: test/main.c
	gcc -g -O2 -c $^ -o $@

$(RUSTLIB):
	cargo build --quiet --release

.PHONY: $(RUSTLIB)