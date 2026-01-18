#ifndef LIB_H
#define LIB_H

struct vec2 {
    union {
        struct {
            float x; float y;
        };
        float at[2];
    };
};
struct vec4 {
    union {
        struct {
            float x; float y; float z; float w;
        };
        float at[4];
    };
};

void draw_line(void *context, struct vec2 from, struct vec2 to, float thickness, struct vec4 color, void *closure);

#endif