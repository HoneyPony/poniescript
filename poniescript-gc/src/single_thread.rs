use std::{alloc::{self, Layout}, collections::VecDeque, marker::PhantomData, mem::MaybeUninit, os::raw::c_void, sync::atomic::Ordering};

use crate::{GC_ALLOCATE_MARKED, GcFrame, Gp, HasPsHeader, HasPsType};

extern "C" {
    fn poni_gc_visit_object(gc: &mut Gc, ptr: *mut u64);
    fn poni_gc_visit_roots(gc: &mut Gc);
    fn poni_gc_get_allocation_size(ptr: *mut u64) -> usize;
}

/// The object responsible for marking and sweeping.
pub struct Gc {
    queue: VecDeque<*mut u64>,
}

/// The handle used to create GcContext. (?)
pub struct GcHandle {
    has_created: bool,
}

impl GcHandle {
    #[unsafe(export_name = "poni_gc_create_context_for_existing")]
    pub fn create_context_for_existing(&mut self) -> Box<GcContext<'_>> {
        if self.has_created {
            panic!("poni_gc: in single-threaded mode, can only create one GcContext.");
        }

        self.has_created = true;
        Box::new(GcContext {
             frame_list: std::ptr::null(),
             shared: Box::new(GcShared {}),
             own_allocs: Vec::new(),
             total_len: 0,
             // Set initial collection threshold. Set it to a few MB for now,
             // as at leastfor now we are targetting wasm.
             collect_threshold: 64,
             gc: Gc::new(),
             phantom: PhantomData,
        })
    }
}

// Shared state.
struct GcShared {

}

/// The per-thread handle to the Gc. Handles allocation and safepointing.

#[repr(C)]
pub struct GcContext<'shared> {
    /// The thread's GcFrame list. Must be the first pointer in this struct.
    frame_list: *const GcFrame,
    /// The opaque shared struct. Must be the second member.
    shared: Box<GcShared>,
    
    own_allocs: Vec<*mut u64>,
    total_len: usize,
    collect_threshold: usize,

    // Because we're single threaded, we can just have our own Gc.
    gc: Gc,

    phantom: PhantomData<&'shared ()>
}

impl Gc {
    /// Called by the poni_gc_visit_object function.
    #[unsafe(export_name = "poni_gc_mark")]
    pub fn mark(&mut self, object: *mut u64) {
        let is_marked = unsafe { *object & 1 != 0 };

        if is_marked {
            return;
        }

        unsafe {
            *object |= 1;
            self.queue.push_front(object);
        }
    }

    fn new() -> Self {
        Gc {
            queue: VecDeque::new(),
        }
    }

    fn collect(&mut self) {
        unsafe {
            poni_gc_visit_roots(self);

            while let Some(next) = self.queue.pop_back() {
                poni_gc_visit_object(self, next);
            }
        }
    }
}

impl<'shared> GcContext<'shared> {
    fn collect(&mut self) {
        let mut frame = self.frame_list;

        log::info!("ctx {:?}: begin scan", self as *const _);

        while !frame.is_null() {
            // Dereference the inner frame: We have checked that it's not NULL.
            let inner_frame = unsafe { &*frame };

            //eprintln!("poni-gc: do_scan: reading from frame {:?}", ptr::addr_of!(inner_frame));

            // Push all of the pointers from the frame into our queue.
            for i in 0..inner_frame.pointer_count {
                let candidate = inner_frame.read_ptr(i as usize);

                // Skip null pointers.
                if !candidate.is_null() {
                    // Skip objects that have already been marked.
                    if unsafe { *candidate & 1 == 0 } {
                        log::info!("ctx {:?}: found mark candidate {:?}", self as *const _, candidate);
                        self.gc.queue.push_front(candidate);
                    }
                }
            }

            // Walk down the list.
            frame = inner_frame.prev;
        }

        log::info!("ctx {:?}: scan finished", self as *const _);

        // Fire off the gc.
        self.gc.collect();

        // Now sweep. (We are responsible for this).
        let mut new_allocs = Vec::new();
        let mut total_freed = 0;
        for ptr in &self.own_allocs {
            let ptr = *ptr;
            
            // Free & skip any alloccations that aren't marked.
            if unsafe { *ptr & 1 == 0 } {
                unsafe { 
                    let size = poni_gc_get_allocation_size(ptr);
                    total_freed += size;
                    let layout = Layout::from_size_align(size, align_of::<u64>()).unwrap();
                    log::info!("poni-gc: freeing {:?} ({} bytes, tag {:x})", ptr, size, *ptr);
                    alloc::dealloc(ptr as *mut u8, layout);
                }

                // Don't add this allocation to the new_allocations list.
                continue;
            }

            // Clear the bit.
            unsafe { *ptr &= !1; }
            new_allocs.push(ptr);
        }

        self.own_allocs = new_allocs;
        self.total_len -= total_freed;
        // Very janky update to collect threshold.
        if self.total_len > self.collect_threshold / 2 {
            self.collect_threshold = self.total_len * 2;
            log::info!("poni-gc: new collect threshold {}", self.collect_threshold);
        }
    }

    #[unsafe(export_name = "poni_gc_poll_slow")]
    pub fn poll_slow(&mut self) {
        if self.total_len > self.collect_threshold {
            self.gc.collect();
            self.total_len = 0
        }
    }

    #[unsafe(export_name = "poni_gc_alloc")]
    pub fn alloc_raw_bytes(&mut self, size: usize) -> *mut u64 {
        let layout = Layout::from_size_align(size, align_of::<u64>()).unwrap();
        let ptr = unsafe { alloc::alloc(layout) };
        let ptr = ptr as *mut u64;

        // We must always reset the first 8-bytes to 0.
        unsafe { *ptr = 0; }

        if GC_ALLOCATE_MARKED.load(Ordering::Relaxed) {
            log::info!("ctx {:?}: allocated {} bytes (marked)", self as *const _, size);
            unsafe { *ptr |= 1; }
        }
        else {
            log::info!("ctx {:?}: allocated {} bytes (unmarked)", self as *const _, size);
        }

        self.own_allocs.push(ptr);

        self.total_len += size;

        ptr
    }

    pub fn alloc<T: HasPsHeader + HasPsType>(&mut self, init: T) -> Gp<T> {
        let ptr = self.alloc_raw_bytes(std::mem::size_of::<T>());

        let as_maybe_uninit = ptr as *mut MaybeUninit<T>;
        // SAFETY: We know the pointer is valid (if alloc_raw_bytes is implemented
        // correctly), so we can safely dereference it.
        let as_maybe_uninit = unsafe { &mut *as_maybe_uninit };

        let as_init = as_maybe_uninit.write(init);

        // SAFETY: This is a valid pointer.
        unsafe { Gp::from_ptr(as_init as *mut T) }
    }
}

#[unsafe(export_name = "poni_gc_spawn")]
pub extern "C" fn gc_spawn() -> Box<GcHandle> {
    let handle = GcHandle {
        // Can create one more handle.
        has_created: false,
    };

    Box::new(handle)
}