#include "lib.h"

#include <stdio.h>

void draw_line(void *context, struct vec2 from, struct vec2 to, float thickness, struct vec4 color, void *closure) {
    printf("%f %f\n", from.x, from.y);
    printf("%f %f\n", to.x, to.y);
    printf("%f\n", thickness);
    printf("%f %f %f %f\n", color.x, color.y, color.z, color.w);
}

void caller();

int main() {
    caller();
}