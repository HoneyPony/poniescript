mod core;
mod array;
mod vec;
mod fmt;
mod type_ids;

#[cfg(feature = "entry")]
mod entry;

pub use core::*;
pub use array::*;
pub use type_ids::*;
pub use vec::*;

pub use poniescript_gc::GcContext;

#[cfg(test)]
#[unsafe(export_name = "poni_gc_visit_object")]
extern "C" fn test_visit_object(_gc: &mut poniescript_gc::Gc, _ptr: *mut u64) {}

#[cfg(test)]
#[unsafe(export_name = "poni_gc_visit_roots")]
extern "C" fn test_visit_roots(_gc: &mut poniescript_gc::Gc) {}

#[cfg(test)]
#[unsafe(export_name = "poni_gc_get_allocation_size")]
extern "C" fn test_allocation_size(_gc: &mut poniescript_gc::Gc) -> usize { 8 }
