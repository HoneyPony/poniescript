CC=clang
#GAMELIB=../target/debug/ponygame_mini
GAMERUST=../target/debug/ponygame-mini

# game: pony.o $(GAMELIB)
# 	$(CC) -fuse-ld=mold $^ -o $@ -lm

all: game-host game-script.so

hot-a: pony.o
	$(CC) -fuse-ld=mold $^ -o game-script-A.so -shared
hot-b: pony.o
	$(CC) -fuse-ld=mold $^ -o game-script-B.so -shared

game-host: $(GAMERUST)
	cp $< $@

game-script.so: pony.o
	$(CC) -fuse-ld=mold $^ -o $@ -shared

$(GAMERUST): src/main.rs
	RUSTFLAGS="-C link-arg=-rdynamic" cargo build

pony.o: pony.c
	$(CC) -g -c $^ -o $@ -I../poniescript
	
pony.c: test.poni
	poniescript -e test.poni -i ponygame.h -o $@
