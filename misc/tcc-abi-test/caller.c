#include "lib.h"

void caller() {
    draw_line(NULL, ps_mk_vec2(10, 20), ps_mk_vec2(30, 40), 50, ps_mk_vec4(60, 70, 80, 90), NULL);
}