#ifndef PONI_GLUE_H
#define PONI_GLUE_H

/**
 * Glue API for PonieScript.
 * 
 * Used for writing C APIs that can easily interact with PonieScript. The idea
 * is that PonieScript supports a partial C parser, and extracts PonieScript
 * definitions from a C header file.
 * 
 * To provide sufficient information to PonieScript about each definition
 * provided this way, the poni_glue.h header defines a series of macros
 * that add additional information about some C declaration.
 */

/**
 * Define a PonieScript class. This should annotate a struct type that has
 * a struct ps_object object field as its first member.
 * 
 * Example:
 *     PS_CLASS("Sprite")
 *     struct sprite {
 *         struct ps_object object;
 *     }
 */
#define PS_CLASS(name)

/**
 * Defines a class member or a global variable. Should come before the variable
 * declaration. Leave the arguments empty to have PonieScript infer the name.
 * 
 * Example:
 *     PS_VAR("total_ticks") extern ps_int total_ticks;
 * 
 * Example:
 *     PS_CLASS("Horse")
 *     struct horse {
 *         struct ps_object object;
 * 
 *         PS_VAR("coat_color") ps_vec4 my_coat_color;
 *         PS_VAR() ps_float height;
 *     }
 */
#define PS_VAR(name)

/**
 * Defines a global function or static class function. Should come before the
 * function declaration. Leave the arguments empty to have PonieScript infer
 * the name.
 * 
 * IMPORTANT: Any function callable from PonieScript must conform to the ABI
 * (we do not yet support raw functions). This means the first parameter must
 * be a void pointer.
 * 
 * Example:
 *     PS_FUN() ps_bool get_key_state(void*, ps_int keycode);
 * 
 * Example:
 *     PS_FUN("exit") void exit_shim(void*, ps_int code) { exit(code); }
 */
#define PS_FUN(name)

/**
 * Defines a class member function. Should come before the function declaration.
 * The class_name argument must be provided, and can be nested ("Class1.Class2").
 * Leave the name argument empty to have PonieScript infer the function name.
 * 
 * Example:
 *     PS_METHOD("Sprite",) ps_vec2 get_dimensions(void *self) { Sprite *s = self; ... }
 */
#define PS_METHOD(class_name, name)

#endif