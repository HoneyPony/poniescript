time (target/release/poniescript \
    -c tcc \
    -o out.o -o out2.o -o out3.o -o out4.o \
    -C-Iponiescript --codegen-threads 4 \
    big5.poni && clang -fuse-ld=mold -o out out.o out2.o out3.o out4.o -lponiescript_gc -Ltarget/debug)