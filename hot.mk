# Used for demoing poni_run functionality.
# Should maybe eventually be moved..?

all: game.so

game.so: misc/run/poni_run.poni
	poniescript -e --hot misc/run/poni_run.poni -o game.c --no-timing
	gcc -shared game.c -o game.so -g -fPIC
