#ifndef PONYQUAD_H
#define PONYQUAD_H

#include "poni/poni.h"
#include "poni/poni_glue.h"

PS_FUN() void clear_background(PONI_ABI(ps_vec4 color));

PS_FUN() void draw_line(PONI_ABI(ps_vec2 from, ps_vec2 to, ps_float thickness, ps_vec4 color));
PS_FUN() void draw_rectangle(PONI_ABI(ps_vec2 at, ps_vec2 size, ps_vec4 color));
PS_FUN() ps_vec3 draw_text(PONI_ABI(ps_strbuf *text, ps_vec2 at, ps_float size, ps_vec4 color));

PS_FUN() ps_float screen_width(PONI_ABI());
PS_FUN() ps_float screen_height(PONI_ABI());

PS_CLASS("Texture2D")
struct texture2d {
    ps_object header;

    // This type is not constructible...
    char opaque[16];
};

PS_FUN() struct texture2d* load_texture(PONI_ABI(ps_strbuf *path));
PS_FUN() void draw_texture(PONI_ABI(struct texture2d* texture, ps_vec2 position, ps_vec4 color));

PS_FUN() ps_bool is_key_down(PONI_ABI(ps_int key));
PS_FUN() ps_bool is_key_pressed(PONI_ABI(ps_int key));
PS_FUN() ps_bool is_key_released(PONI_ABI(ps_int key));

PS_FUN() ps_bool is_mouse_button_down(PONI_ABI(ps_int button));
PS_FUN() ps_bool is_mouse_button_pressed(PONI_ABI(ps_int button));
PS_FUN() ps_bool is_mouse_button_released(PONI_ABI(ps_int button));

PS_FUN() ps_vec2 mouse_position(PONI_ABI());
PS_FUN() ps_vec2 mouse_position_local(PONI_ABI());
PS_FUN() ps_vec2 mouse_delta_position(PONI_ABI());
PS_FUN() ps_vec2 mouse_wheel(PONI_ABI());

PS_FUN() ps_float get_frame_time(PONI_ABI());
PS_FUN() ps_float get_time(PONI_ABI());
PS_FUN() ps_int get_fps(PONI_ABI());

PS_FUN() ps_vec3 hsl_to_rgb(PONI_ABI(ps_vec3 hsl));
PS_FUN() ps_vec4 hsla_to_rgba(PONI_ABI(ps_vec4 hsla));
PS_FUN() ps_vec3 rgb_to_hsl(PONI_ABI(ps_vec3 rgb));
PS_FUN() ps_vec4 rgba_to_hsla(PONI_ABI(ps_vec4 rgba));

#endif