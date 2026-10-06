use poniescript_gc::PsFloat;

#[unsafe(no_mangle)]
pub extern "C" fn ps_mod_float(a: PsFloat, b: PsFloat) -> PsFloat {
    a.rem_euclid(b)
}