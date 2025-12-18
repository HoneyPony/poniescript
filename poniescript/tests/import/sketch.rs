



#[repr(C)]
#[poni_class]
struct Test {
    // Should the object header be intrusive?
    header: PoniHeader,

    #[poni_var]
    value: PsInt,

    #[poni_var]
    other_value: PsInt,
}

#[poni_fun]
#[unsafe(no_mangle)]
// Should we have to manually specify the ABI...?
fn add(ctx: &GcCtx, x: PsInt, y: PsInt, closure: *const c_void) -> PsInt {

}

// Could also have a macro to automatically generate wrapper functions.
// Have to provide the symbol name for the wrapped function...
#[poni_fun_wrap("l_add")]
fn add(x: PsInt, y: PsInt) -> PsInt {

}

// With a generating macro, we can also easily (?) implement member functions...
impl Test {
    #[poni_fun_wrap("l_Test_do_stuff")]
    fn do_stuff(&self, x: PsInt) {

    }
}