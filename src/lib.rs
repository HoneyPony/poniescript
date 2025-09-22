use std::{alloc::{self, Layout}, collections::VecDeque, ptr, sync::{atomic::{AtomicU64, Ordering}, Condvar, Mutex}};

extern "C" {
    fn poni_gc_visit_object(gc: &mut Gc, ptr: *mut u64);
    fn poni_gc_visit_roots(gc: &mut Gc);
}

const GC_FLAG_SCAN: u64 = 2;
const GC_FLAG_NOP: u64 = 1;

struct Gc {
    mark_queue: VecDeque<*mut u64>,
    shared: GcShared,
}

struct GcAllocator {
    allocations: Vec<*mut u64>,
    allocate_marked: bool,
}

struct GcShared {
    queue_queue: Mutex<Vec<Vec<*mut u64>>>,

    gc_flags: AtomicU64,

    available_threads: u64,

    outstanding_threads: Mutex<u64>,
    outstanding_thread_cv: Condvar,

    allocator: Mutex<GcAllocator>,
}

#[repr(C)]
struct GcFrame {
    prev: *const GcFrame,
    pointer_count: u64,
    pointers: [*mut u64; 0]
}

#[repr(C)]
struct GcContext<'a> {
    frame_list: *const GcFrame,
    shared: &'a GcShared,
}

impl<'a> GcContext<'a> {
    pub extern "C" fn poni_gc_alloc(&mut self, size: usize) -> *mut u64 {
        self.shared.alloc(size)
    }

    fn do_scan(&self) {
        let mut queue = Vec::new();
        let mut frame = self.frame_list;

        while !frame.is_null() {
            // Dereference the inner frame: We have checked that it's not NULL.
            let inner_frame = unsafe { &*frame };

            // Push all of the pointers from the frame into our queue.
            for i in 0..inner_frame.pointer_count {
                let candidate = inner_frame.read_ptr(i as usize);

                // Skip null pointers.
                if !candidate.is_null() {
                    // Skip objects that have already been marked.
                    if unsafe { *candidate & 1 != 0 } {
                        queue.push(candidate);
                    }
                }
            }

            // Walk down the list.
            frame = inner_frame.prev;
        }

        let mut lock = self.shared.queue_queue.lock().unwrap();
        lock.push(queue);
    }

    #[no_mangle]
    extern "C" fn poni_gc_poll_slow(&self) {
        let do_scan = self.shared.gc_flags.load(Ordering::Relaxed) & GC_FLAG_SCAN != 0;
        if do_scan {
            self.do_scan();
        }

        let mut lock = self.shared.outstanding_threads.lock().unwrap();
        *lock -= 1;
        if *lock == 0 {
            self.shared.outstanding_thread_cv.notify_one();
        }
    }
}

impl GcFrame {
    fn read_ptr(&self, idx: usize) -> *mut u64 {
        if idx >= self.pointer_count as usize {
            panic!("Invalid index in GCFrame");
        }
        self.pointers[idx]
    }
}

impl GcShared {
    pub fn new() -> Self {
        GcShared {
            queue_queue: Mutex::new(Vec::new()),
            gc_flags: AtomicU64::new(0),
            available_threads: 0,
            outstanding_threads: Mutex::new(0),
            outstanding_thread_cv: Condvar::new(),
            allocator: Mutex::new(GcAllocator::new()),
        }
    }

    pub fn alloc(&self, size: usize) -> *mut u64 {
        let mut allocator = self.allocator.lock().unwrap();
        allocator.alloc(size)
    }
}

impl GcAllocator {
    pub fn new() -> Self {
        GcAllocator {
            allocations: Vec::new(),
            allocate_marked: false,
        }
    }

    pub fn alloc(&mut self, size: usize) -> *mut u64 {
        let layout = Layout::from_size_align(size, align_of::<u64>()).unwrap();
        let ptr = unsafe { alloc::alloc(layout) };
        let ptr = ptr as *mut u64;

        if self.allocate_marked {
            unsafe { *ptr |= 1; }
        }

        self.allocations.push(ptr);

        ptr
    }

    pub fn sweep(&mut self) {
        // TODO: Maybe build the new_allocations array as part of marking? That
        // should save some time.
        let mut new_allocations: Vec<*mut u64> = vec![];
        for alloc in &self.allocations {
            let alloc = *alloc;

            // Skip any alloccations that aren't marked.
            if unsafe { *alloc & 1 == 0 } { continue; }

            // Otherwise, clear the mark bit.
            unsafe { *alloc &= !1; }
            new_allocations.push(alloc);
        }
        self.allocations = new_allocations;
    }
}

impl Gc {
    fn handshake(&self, flags: u64) {
        let mut outstanding = self.shared.outstanding_threads.lock().unwrap();

        *outstanding = self.shared.available_threads;
        self.shared.gc_flags.fetch_update(Ordering::SeqCst, Ordering::SeqCst, 
            |f| Some(f | flags)).unwrap();

        while *outstanding > 0 {
            outstanding = self.shared.outstanding_thread_cv.wait(outstanding).unwrap();
        }
    }

    pub fn collect(&mut self) {
        {
            let mut allocator = self.shared.allocator.lock().unwrap();
            allocator.allocate_marked = true;
        }

        self.handshake(GC_FLAG_NOP);

        unsafe { poni_gc_visit_roots(self); }

        loop {
            self.handshake(GC_FLAG_SCAN);

            // Move objects from queue_queue to regular queue
            let queue_queue: Vec<_> = {
                let mut lock = self.shared.queue_queue.lock().unwrap();
                std::mem::take(&mut lock)
            };

            for queue in queue_queue {
                for ptr in queue {
                    // Mark every pointer in the queue.
                    self.poni_gc_mark(ptr);
                }
            }

            // Once we reach a fixed point (no new objects to mark from the
            // stack scans), we are done.
            if self.mark_queue.is_empty() {
                break;
            }

            // Otherwise, perform the graph travesal.
            self.process_queue();
        }

        self.sweep();
    }

    pub fn process_queue(&mut self) {
        loop {
            let Some(next) = self.mark_queue.pop_back() else { break; };

            // We assume objects that are in the mark queue have already been
            // marked, and so will unconditionally be visited.
            unsafe { poni_gc_visit_object(self, next); }
        }
    }

    /// gc_mark is the fundamental 'mark' function that must be called by
    /// poni_gc_visit_object or poni_gc_visit_roots.
    #[no_mangle]
    pub extern "C" fn poni_gc_mark(&mut self, ptr: *mut u64) {
        let is_marked = unsafe { *ptr & 1 != 0 };

        // Object already marked: Nothing to do.
        if is_marked { return; }

        // Object is not marked: Mark it now, and visit it when we get a chance.
        unsafe {
            *ptr |= 1;

            self.mark_queue.push_front(ptr);
        }
    }

    fn sweep(&mut self) {
        let mut allocator = self.shared.allocator.lock().unwrap();
        allocator.sweep();
        allocator.allocate_marked = false;
    }

    #[no_mangle]
    pub extern "C" fn poni_gc_spawn() -> Box<Gc> {
        Box::new(Gc { mark_queue: VecDeque::new(), shared: GcShared::new() })
    }
}