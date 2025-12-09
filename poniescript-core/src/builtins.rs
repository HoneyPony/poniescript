use std::sync::Arc;

use crate::db::BuiltinMethodPtr;

pub mod dynarray;
pub mod option;

pub struct BuiltinMethodTable {
	pub dynarray_push: BuiltinMethodPtr,
    pub option_unwrap: BuiltinMethodPtr,
}

impl BuiltinMethodTable {
	pub fn new() -> Self {
        Self {
            dynarray_push: Arc::new(dynarray::DynarrayPush),
            option_unwrap: Arc::new(option::OptionUnwrap),
        }
    }
}