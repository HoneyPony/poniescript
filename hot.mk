# For some reason, it seems like to get the reloads to properly work, you have
# to reload twice.
all: game.so

game.so: misc/run/poni_run.poni
	poniescript -e --hot misc/run/poni_run.poni -o game.c --no-timing
	gcc -shared game.c -o game.so -g -fPIC

a:
	poniescript -e --hot misc/run/poni_run.poni -o game-a.c
	gcc -shared game-a.c -o game-a.so -g -fPIC

b:
	poniescript -e --hot misc/run/poni_run.poni -o game-b.c
	gcc -shared game-a.c -o game-b.so -g -fPIC

.PHONY: a b