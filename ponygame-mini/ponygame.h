#ifndef PONYGAME_H
#define PONYGAME_H

#include "poni/poni.h"
#include "poni/poni_glue.h"

/* PS_BEGIN_INTERFACE

@cimport("pg_sprite")
class Sprite {
    @cimport("width")
    var width: int;
    @cimport("height")
    var height: int;
}

PS_END_INTERFACE */

PS_CLASS("Sprite")
struct pg_sprite {
    struct ps_object object;

    PS_VAR() ps_int width;
    PS_VAR() ps_int height;
};

PS_FN()
void
draw_sprite(void *context, struct pg_sprite *sprite, ps_vec2 where) {
    
}

PS_METHOD("Sprite.get_dimensions",)
ps_vec2
Sprite_get_dimensions(void *self) {
    struct pg_sprite *this = self;
    return (ps_vec2){ .x = this->width, .y = this->height };
}

#endif