RUSTLIB=target/release/libponi_gc_rs.a

testgc: test/main.o $(RUSTLIB)
	gcc test/main.o $(RUSTLIB) -o $@

test/main.o: test/main.c
	gcc -c $^ -o $@

$(RUSTLIB):
	cargo build --quiet --release

.PHONY: $(RUSTLIB)