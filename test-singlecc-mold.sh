time (target/release/poniescript \
    -c tcc \
    -o out.o \
    -C-Iponiescript --codegen-threads 4 \
    "${1:-big5.poni}" && clang -fuse-ld=mold -o out out.o -lponiescript_gc -Ltarget/debug)