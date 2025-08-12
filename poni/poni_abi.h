#ifndef PONI_ABI_H
#define PONI_ABI_H

struct ps_context;

#define PS_ABI(...) struct ps_context *ps_ctx, void *ps_this __VA_OPT__(,) __VA_ARGS__

#define PS_GET_THIS(Ty) Ty *this = ps_this

#define PS_CALL(fn, this, ...) fn(ps_ctx, this __VA_OPT__(,) __VA_ARGS__)

#endif