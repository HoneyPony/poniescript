#ifndef LIB_H
#define LIB_H

struct vec2 {
    union {
        struct {
            float x; float y;
        };
        
        struct {
            float v0; float v1;
        };

        // If we comment out these arrays everything seems to work correctly.
        // So, not all unions cause problems, just ones with arrays...?
        float at[2];
    };
};
struct vec4 {
    union {
        struct {
            float x; float y; float z; float w;
        };
        
        struct {
            float v0; float v1; float v2; float v3;
        };

        float at[4];
    };
};

void draw_line(void *context, struct vec2 from, struct vec2 to, float thickness, struct vec4 color, void *closure);

#endif