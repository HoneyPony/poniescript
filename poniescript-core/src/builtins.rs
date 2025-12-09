use std::sync::Arc;

use crate::db::BuiltinMethodPtr;

pub mod dynarray;

pub struct BuiltinMethodTable {
	pub dynarray_push: BuiltinMethodPtr,
}

impl BuiltinMethodTable {
	pub fn new() -> Self {
        Self {
            dynarray_push: Arc::new(dynarray::DynarrayPush)
        }
    }
}