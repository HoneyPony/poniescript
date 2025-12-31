#ifndef PONI_ABI_H
#define PONI_ABI_H

struct ps_context;

// Right now, the PS_ABI is args, then closure. This might change.
#define PS_ABI(...) __VA_ARGS__ __VA_OPT__(,) void *ps_this

// #define PS_ABI(...) struct ps_context *ps_ctx, void *ps_this __VA_OPT__(,) __VA_ARGS__

#define PS_GET_THIS(Ty) Ty *this = ps_this

#define PS_CALL(fn, this, ...) fn(ps_ctx, this __VA_OPT__(,) __VA_ARGS__)

#endif