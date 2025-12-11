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

    pub option_unwrap: BuiltinMethodPtr,

    pub vec_map: BuiltinMethodPtr,
}

impl BuiltinMethodTable {
	pub fn new() -> Self {
        Self {
            dynarray_push: Arc::new(dynarray::DynarrayPush),
            dynarray_any: Arc::new(dynarray::DynarrayAny { all: false }),
            dynarray_all: Arc::new(dynarray::DynarrayAny { all: true }),
            dynarray_clone_shallow: Arc::new(dynarray::DynarrayCloneShallow),

            option_unwrap: Arc::new(option::OptionUnwrap),

            vec_map: Arc::new(vec::VecMap),
        }
    }
}