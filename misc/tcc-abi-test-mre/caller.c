#include "lib.h"

#include <stddef.h>

void caller() {
    draw_line(NULL, (struct vec2){.x=10, 20}, (struct vec2){30, 40}, 50, (struct vec4){60, 70, 80, 90}, NULL);
}