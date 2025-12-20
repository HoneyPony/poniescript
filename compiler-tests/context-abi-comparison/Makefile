CC=gcc
CFLAGS=-O2

main: harness.o thread_local.o parameter.o
	$(CC) $^ -o $@ $(CFLAGS)

%.o: %.c
	$(CC) -c $^ -o $@ $(CFLAGS)

clean:
	rm *.o
	rm main

.PHONY: clean
