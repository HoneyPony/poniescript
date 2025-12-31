time (target/release/poniescript \
    -c tcc \
    --prelude-h ./out.h \
    -o out.o -o out2.o -o out3.o -o out4.o -o out5.o -o out6.o -o out7.o -o out8.o \
    -C-Iponiescript --codegen-threads 8 \
    "${1:-big5.poni}" && clang -fuse-ld=mold -o out \
    out.o out2.o out3.o out4.o out5.o out6.o out7.o out8.o \
    -lponiescript_gc -Ltarget/debug)