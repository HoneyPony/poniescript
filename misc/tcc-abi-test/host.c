#include "lib.h"

void draw_line(PONI_ABI(ps_vec2 from, ps_vec2 to, ps_float thickness, ps_vec4 color)) {
    printf("%f %f\n", from.x, from.y);
    printf("%f %f\n", to.x, to.y);
    printf("%f\n", thickness);
    printf("%f %f %f %f\n", color.x, color.y, color.z, color.w);
}

void caller();

int main() {
    caller();
}