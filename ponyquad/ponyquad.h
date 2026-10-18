#ifndef PONYQUAD_H
#define PONYQUAD_H

#include "poni/poni.h"
#include "poni/poni_glue.h"

/// Clear the screen, filling it with the given color.
PS_FUN() void clear_background(PONI_ABI(ps_vec4 color));

/// Draw a solid line between the two points.
PS_FUN() void draw_line(PONI_ABI(ps_vec2 from, ps_vec2 to, ps_float thickness, ps_vec4 color));

/// Fill a rectangle with the given color, at the given location, with the given
/// size.
PS_FUN() void draw_rectangle(PONI_ABI(ps_vec2 at, ps_vec2 size, ps_vec4 color));


/// Returns the width of the screen.
PS_FUN() ps_float screen_width(PONI_ABI());
/// Returns the height of the screen.
PS_FUN() ps_float screen_height(PONI_ABI());

/// A 2 dimensional texture that can be drawn to the screen. Lives on the GPU
/// and so cannot be directly modified.
PS_CLASS("Texture2D")
struct texture2d {
    ps_object header;

    // This type is not constructible...
    char opaque[16];
};

PS_CLASS("Font")
struct font {
    ps_object header;

    // Technically only needs to be 24 but I will be safe.
    char opqaue[64];
};

PS_CLASS("Camera2D")
struct camera2d {
    ps_object header;

    PS_VAR() ps_float rotation;
    PS_VAR() ps_vec2 zoom;
    PS_VAR() ps_vec2 target;
    PS_VAR() ps_vec2 offset;

    PS_VAR() PS_OPTION(struct camera_viewport*) viewport;
};

PS_CLASS("CameraViewport")
struct camera_viewport {
    ps_object header;

    PS_VAR() ps_vec2 offset;
    PS_VAR() ps_vec2 size;
};

PS_CLASS("Sound")
struct sound {
    ps_object header;

    char opaque[64];
};

PS_CLASS("StringLoader")
struct string_loader {
    ps_object header;

    char opaque[64];
};

PS_FUN() struct sound* load_sound(PONI_ABI(ps_strbuf *path));
PS_FUN() void play_sound_once(PONI_ABI(struct sound *sound));
PS_FUN() void play_sound_looping(PONI_ABI(struct sound *sound));
PS_FUN("stop_sound") void pq_stop_sound(PONI_ABI(struct sound *sound));
PS_FUN("is_sound_loaded") ps_bool pq_is_sound_loaded(PONI_ABI(struct sound *sound));

PS_FUN() struct font* load_font(PONI_ABI(ps_strbuf *path));
/// Draw text to the screen, using the given font size and color.
PS_FUN() ps_vec3 draw_text(PONI_ABI(ps_strbuf *text, ps_vec2 at, ps_float size, ps_vec4 color));
/// Draw text to the screen, using the given font size and color.
PS_FUN() ps_vec3 draw_texts(PONI_ABI(ps_strbuf *text, ps_vec2 at, ps_float size, ps_float scale, ps_vec4 color));
/// Draw text to the screen, using the given font, font size and color.
PS_FUN() ps_vec3 draw_text_font(PONI_ABI(ps_strbuf *text, struct font *font, ps_vec2 at, ps_float size, ps_vec4 color));
/// Draw text to the screen, using the given font, font size and color.
PS_FUN() ps_vec3 draw_text_fonts(PONI_ABI(ps_strbuf *text, struct font *font, ps_vec2 at, ps_float size, ps_float scale, ps_vec4 color));

PS_FUN() ps_vec3 measure_text_font(PONI_ABI(ps_strbuf *text, struct font *font, ps_float size));
PS_FUN() ps_vec3 measure_text_fonts(PONI_ABI(ps_strbuf *text, struct font *font, ps_float size, ps_float scale));

PS_FUN("is_font_loaded") ps_bool pq_is_font_loaded(PONI_ABI(struct font* font));

PS_FUN() void set_camera(PONI_ABI(struct camera2d *camera));
PS_FUN() void set_default_camera(PONI_ABI());

PS_FUN() struct texture2d* load_texture(PONI_ABI(ps_strbuf *path));
PS_FUN() void draw_texture(PONI_ABI(struct texture2d* texture, ps_vec2 position, ps_vec4 color));
PS_FUN() void draw_texture_flip(PONI_ABI(struct texture2d *texture, ps_vec2 position, ps_bool flip_h, ps_bool flip_v, ps_vec4 color));
PS_FUN() void draw_texture_flip_frame(PONI_ABI(struct texture2d *texture, ps_vec2 position, ps_bool flip_h, ps_bool flip_v, ps_vec4 frame, ps_vec4 color));
PS_FUN() void draw_texture_rot(PONI_ABI(struct texture2d* texture, ps_vec2 position, ps_float rotation, ps_vec4 color));
PS_FUN() void draw_texture_rot_scale(PONI_ABI(struct texture2d* texture, ps_vec2 position, ps_float rotation, ps_float scale, ps_vec4 color));

PS_FUN() ps_bool is_key_down(PONI_ABI(ps_int key));
PS_FUN() ps_bool is_key_pressed(PONI_ABI(ps_int key));
PS_FUN() ps_bool is_key_released(PONI_ABI(ps_int key));

PS_FUN() ps_bool is_mouse_button_down(PONI_ABI(ps_int button));
PS_FUN() ps_bool is_mouse_button_pressed(PONI_ABI(ps_int button));
PS_FUN() ps_bool is_mouse_button_released(PONI_ABI(ps_int button));

PS_FUN() ps_vec2 mouse_position(PONI_ABI());
PS_FUN() ps_vec2 mouse_position_local(PONI_ABI());
PS_FUN() ps_vec2 mouse_delta_position(PONI_ABI());
PS_FUN("mouse_wheel") ps_vec2 pq_mouse_wheel(PONI_ABI());

PS_FUN() ps_float get_frame_time(PONI_ABI());
PS_FUN() ps_float get_time(PONI_ABI());
PS_FUN() ps_int get_fps(PONI_ABI());

PS_FUN() ps_vec3 hsl_to_rgb(PONI_ABI(ps_vec3 hsl));
PS_FUN() ps_vec4 hsla_to_rgba(PONI_ABI(ps_vec4 hsla));
PS_FUN() ps_vec3 rgb_to_hsl(PONI_ABI(ps_vec3 rgb));
PS_FUN() ps_vec4 rgba_to_hsla(PONI_ABI(ps_vec4 rgba));

/// Sets a 2D canvas (using the macroquad_canvas crate). This is good for games
/// with a fixed resolution.
PS_FUN() void set_canvas(PONI_ABI(ps_vec2 size));
/// Clears any 2D canvas set by set_canvas().
PS_FUN() void clear_canvas(PONI_ABI());

/// Sets the default filter mode for textures.
PS_FUN() void set_default_filter_mode(PONI_ABI(ps_int mode));

// Until such time as these are language builtins, provide some useful math
// functions.

/// Length of a vec2.
PS_FUN("len2") ps_float pq_len_vec2(PONI_ABI(ps_vec2 v));
/// Length of a vec3.
PS_FUN("len3") ps_float pq_len_vec3(PONI_ABI(ps_vec2 v));
/// Length of a vec4.
PS_FUN("len4") ps_float pq_len_vec4(PONI_ABI(ps_vec2 v));

/// Normalize a vec2.
PS_FUN("norm2") ps_vec2 pq_norm_vec2(PONI_ABI(ps_vec2 v));
/// Normalize a vec3.
PS_FUN("norm3") ps_vec3 pq_norm_vec3(PONI_ABI(ps_vec2 v));
/// Normalize a vec4.
PS_FUN("norm4") ps_vec4 pq_norm_vec4(PONI_ABI(ps_vec2 v));
/// Get the atan2 (angle) of a given vector. Unlike some atan2 implementations,
/// this is in x, y order (e.g. you can pass a vector to get its angle).
PS_FUN("atan2") ps_float pq_atan2(PONI_ABI(ps_vec2 xy));

/// Gets the power of a base raised to an exponent.
///
/// TODO: Make this a PonieScript builtin.
PS_FUN("pow") ps_float pq_pow(PONI_ABI(ps_float base, ps_float exp));

/// Rounds a vec2 into a vec2i.
PS_FUN("round2") ps_vec2i pq_round2(PONI_ABI(ps_vec2 v));

/// Rounds a float into an int.
PS_FUN("round") ps_int pq_round(PONI_ABI(ps_float f));

/// Floors a float into an int.
PS_FUN("floor") ps_int pq_floor(PONI_ABI(ps_float f));

// GL functions
PS_FUN("gl_push_rotation") void pq_gl_push_rotation(PONI_ABI(ps_float rotation));
PS_FUN("gl_push_scale") void pq_gl_push_scale(PONI_ABI(ps_vec2 scale));
PS_FUN("gl_push_translation") void pq_gl_push_translation(PONI_ABI(ps_vec2 translation));
PS_FUN("gl_pop") void pq_gl_pop(PONI_ABI());

// Random functions

PS_FUN("seed_random_with_time") void pq_seed_random_with_time(PONI_ABI());

/// Inclusive range
PS_FUN("randi_range") ps_int pq_randi_range(PONI_ABI(ps_int low, ps_int high));
PS_FUN("randf_range") ps_float pq_randf_range(PONI_ABI(ps_float low, ps_float high));

PS_CLASS("Map")
struct pq_map {
    ps_object header;

    char opaque[400];
};

PS_FUN() struct string_loader* load_string(PONI_ABI(ps_strbuf *path));
PS_FUN() PS_OPTION(ps_strbuf*) get_loaded_string(PONI_ABI(struct string_loader *loader));
PS_WILL_DEFINE_ITSELF(PS_FUN("load_map") PS_OPTION(struct pq_map*) pq_load_map(PONI_ABI(ps_strbuf *data, PS_ARRAY(ps_strbuf*) tex_name, PS_ARRAY(struct texture2d*) tex_value));)
PS_FUN("draw_map") void pq_draw_map(PONI_ABI(struct pq_map *map, ps_strbuf *layer, ps_vec4 dest));
PS_FUN("get_tile_map_size") ps_vec2i pq_get_tile_map_size(PONI_ABI(struct pq_map *map));
PS_FUN("get_tile_map_tile") ps_int pq_get_tile_map_tile(PONI_ABI(struct pq_map *map, ps_strbuf *layer, ps_vec2i coord));
PS_FUN("set_tile_map_tile") ps_bool pq_set_tile_map_tile(PONI_ABI(struct pq_map *map, ps_strbuf *layer, ps_vec2i coord, ps_int new_id));
PS_FUN("remove_tile_map_tile") ps_bool pq_remove_tile_map_tile(PONI_ABI(struct pq_map *map, ps_strbuf *layer, ps_vec2i coord));

PS_FUN("tiledobj_get_count") ps_int pq_tiledobj_get_count(PONI_ABI(struct pq_map *map, ps_strbuf *layer));
PS_FUN("tiledobj_get_property_float") ps_float pq_tiledobj_get_property_float(PONI_ABI(struct pq_map *map, ps_strbuf *layer, ps_int index, ps_strbuf *property));
PS_FUN("tiledobj_get_property_int") ps_int pq_tiledobj_get_property_int(PONI_ABI(struct pq_map *map, ps_strbuf *layer, ps_int index, ps_strbuf *property));
PS_FUN("tiledobj_get_tile_id") ps_int pq_tiledobj_get_tile_id(PONI_ABI(struct pq_map *map, ps_strbuf *layer, ps_int index));
PS_FUN("tiledobj_get_position") ps_vec2 pq_tiledobj_get_position(PONI_ABI(struct pq_map *map, ps_strbuf *layer, ps_int index));

#endif