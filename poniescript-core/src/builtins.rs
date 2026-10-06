use std::sync::Arc;

use crate::db::BuiltinMethodPtr;

pub mod dynarray;
pub mod option;
pub mod vec;

pub struct BuiltinMethodTable {
	pub dynarray_push: BuiltinMethodPtr,
    pub dynarray_any: BuiltinMethodPtr,
    pub dynarray_all: BuiltinMethodPtr,
    pub dynarray_clone_shallow: BuiltinMethodPtr,
    pub dynarray_pop_or_panic: BuiltinMethodPtr,

    pub option_unwrap: BuiltinMethodPtr,
    pub option_is_some: BuiltinMethodPtr,
    pub option_is_nil: BuiltinMethodPtr,

    pub vec_map: BuiltinMethodPtr,
}

impl BuiltinMethodTable {
	pub fn new() -> Self {
        Self {
            dynarray_push: Arc::new(dynarray::DynarrayPush),
            dynarray_any: Arc::new(dynarray::DynarrayAny { all: false }),
            dynarray_all: Arc::new(dynarray::DynarrayAny { all: true }),
            dynarray_clone_shallow: Arc::new(dynarray::DynarrayCloneShallow),
            dynarray_pop_or_panic: Arc::new(dynarray::DynarrayPopOrPanic),

            option_unwrap: Arc::new(option::OptionUnwrap),
            option_is_some: Arc::new(option::OptionIsSome { invert: false }),
            // Inverse of IsSome is IsNil
            option_is_nil: Arc::new(option::OptionIsSome { invert: true }),

            vec_map: Arc::new(vec::VecMap),
        }
    }
}