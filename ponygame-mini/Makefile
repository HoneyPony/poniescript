CC=clang
GAMELIB=../target/debug/libponygame_mini.a

# game: pony.o $(GAMELIB)
# 	$(CC) -fuse-ld=mold $^ -o $@ -lm

all: game-host game-script.so

game-host: $(GAMELIB)
	$(CC) -rdynamic -fuse-ld=mold $^ -o $@ -lm

game-script.so: pony.o
	$(CC) -fuse-ld=mold $^ -o $@ -shared

$(GAMELIB): src/lib.rs
	RUSTFLAGS="-C link-arg=-rdynamic" cargo build

pony.o: pony.c
	$(CC) -g -c $^ -o $@ -I../poniescript
	
pony.c: test.poni
	poniescript -e test.poni -i ponygame.h -o $@
