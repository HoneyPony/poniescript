mod pointers;
mod primitives;

use std::sync::atomic::{AtomicBool, AtomicU64};

pub use pointers::*;
pub use primitives::*;

#[cfg(not(target_arch = "wasm32"))]
mod concurrent;
#[cfg(target_arch = "wasm32")]
mod single_thread;

#[cfg(not(target_arch = "wasm32"))]
pub use concurrent::*;
#[cfg(target_arch = "wasm32")]
pub use single_thread::*;

#[cfg(feature = "mimalloc")]
#[global_allocator]
static MIMALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

// For now, these are always atomic, even in the single-threaded case.
//
// If we can't do atomics, we can try to move the single-threaded ones to a
// Cell on a shared struct or similar.
#[export_name = "poni_gc_flags"]
static GC_FLAGS: AtomicU64 = AtomicU64::new(0);
static GC_ALLOCATE_MARKED: AtomicBool = AtomicBool::new(false);

/// Shadow-stack GC frame.
/// 
/// These have equivalent representation in single threaded and concurrent case.
#[repr(C)]
struct GcFrame {
    prev: *const GcFrame,
    fn_name: *const i8,
    pointer_count: u64,
}

impl GcFrame {
    fn read_ptr(&self, idx: usize) -> *mut u64 {
        if idx >= self.pointer_count as usize {
            panic!("Invalid index in GCFrame");
        }
        unsafe {
            let addr = self as *const GcFrame;
            let addr = addr.byte_offset(16);
            // Pointer-to-a-pointer
            let addr = addr as *const usize;

            let addr = addr.offset(idx as isize);
            let value = *addr;

            let val = value as *mut u64;

            val
        }
    }
}