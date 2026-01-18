CC=clang
GAMELIB=../target/debug/libponygame_mini.a

game: pony.o $(GAMELIB)
	$(CC) -fuse-ld=mold $^ -o $@ -lm

$(GAMELIB): src/lib.rs
	cargo build

pony.o: pony.c
	$(CC) -g -c $^ -o $@ -I../poniescript
	
pony.c: test.poni
	poniescript -e test.poni -i ponygame.h -o $@
