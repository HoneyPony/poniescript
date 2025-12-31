#include <stdio.h>

// The evidence from this test suggests that tcc is able to parse hexademical
// literals either at the same speed as decimal ones or maybe very slightly
// faster. We will probably want to use hexadecimal literals anyway in order
// to be precise.

int
main() {
    FILE *hex = fopen("literals-hex.c", "w");
    FILE *dec = fopen("literals-dec.c", "w");
    if(!hex || !dec) return 1;

    fprintf(hex, "int main() {\n");
    fprintf(dec, "int main() {\n");
    for(int i = 0; i < 1000000; ++i) {
        fprintf(hex, "\tdouble a%d = 0x0.3p10;\n", i);
        fprintf(dec, "\tdouble a%d = 192.0;\n"   , i);
    }
    fprintf(hex, "}\n");
    fprintf(dec, "}\n");

    fclose(hex);
    fclose(dec);
    return 0;
}