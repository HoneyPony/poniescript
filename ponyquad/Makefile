CC=clang
#GAMELIB=../target/debug/ponygame_mini
GAMERUST=../target/debug/ponyquad

# game: pony.o $(GAMELIB)
# 	$(CC) -fuse-ld=mold $^ -o $@ -lm

all: game-host game-script.so

# hot-a: pony.o
# 	$(CC) -fuse-ld=mold $^ -o game-script-A.so -shared
# hot-b: pony.o
# 	$(CC) -fuse-ld=mold $^ -o game-script-B.so -shared
hot-reload: pony.o
	$(CC) -fuse-ld=mold $^ -o $(OUTLIBNAME) -shared

game-host: $(GAMERUST)
	cp $< $@

game-script.so: pony.o
	$(CC) -fuse-ld=mold $^ -o $@ -shared

$(GAMERUST): src/main.rs src/hot.rs src/bindings.rs
	RUSTFLAGS="-C link-arg=-rdynamic" cargo build --features hotreload

pony.o: pony.c
	$(CC) -fPIC -g -c $^ -o $@ -I../poniescript
	
pony.c: test.poni
	poniescript -e test.poni -i ponyquad.h -o $@

# pony.o: test.poni
# 	poniescript -e test.poni -i ponyquad.h -o $@ -C-I../poniescript -c tcc