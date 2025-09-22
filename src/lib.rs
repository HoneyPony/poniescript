use std::{alloc::{self, Layout}, collections::VecDeque, ptr::{self, null}, sync::{atomic::{AtomicPtr, AtomicU64, Ordering}, Condvar, Mutex}, thread::{self, JoinHandle}, time::Instant};


#[global_allocator]
static MIMALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

extern "C" {
    fn poni_gc_visit_object(gc: &mut Gc, ptr: *mut u64);
    fn poni_gc_visit_roots(gc: &mut Gc);
    fn poni_gc_get_allocation_size(ptr: *mut u64) -> usize;
}

const GC_FLAG_SCAN: u64 = 2;
const GC_FLAG_NOP: u64 = 1;
// Dummy flag to toggle between so that the safepoints recognize there's work to do
const GC_FLAG_DUMMY: u64 = 4;

const GC_REQUEST_COLLECT: u64 = 1;
const GC_REQUEST_SHUTDOWN: u64 = 2;

#[export_name = "poni_gc_flags"]
static GC_FLAGS: AtomicU64 = AtomicU64::new(0);

struct Gc<'a> {
    mark_queue: VecDeque<AtomicPtr<u64>>,
    shared: &'a GcShared,
}

struct GcHandle<'a> {
    join_handle: Option<JoinHandle<()>>,
    shared: &'a GcShared,
}

struct GcAllocator {
    allocations: Vec<AtomicPtr<u64>>,
    allocate_marked: bool,
}

struct GcShared {
    queue_queue: Mutex<Vec<Vec<AtomicPtr<u64>>>>,

    available_threads: Mutex<u64>,

    outstanding_threads: Mutex<u64>,
    outstanding_thread_cv: Condvar,

    allocator: Mutex<GcAllocator>,

    gc_request: Mutex<u64>,
    gc_request_cv: Condvar,

    gc_busy: Mutex<bool>,
    gc_busy_cv: Condvar,

    /// A dummy mutex for sleeping on the gc_flags_cv.
    gc_flags_mutex: Mutex<()>,
    gc_flags_cv: Condvar,
}

#[repr(C)]
struct GcFrame {
    prev: *const GcFrame,
    pointer_count: u64,
}

#[repr(C)]
struct GcContext<'a> {
    frame_list: *const GcFrame,
    shared: &'a GcShared,
    flag: u64,
}

impl<'a> GcContext<'a> {
    #[no_mangle]
    pub extern "C" fn poni_gc_alloc(&mut self, size: usize) -> *mut u64 {
        self.shared.alloc(size)
    }

    fn do_scan(&self) {
        let mut queue = Vec::new();
        let mut frame = self.frame_list;

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
                        queue.push(AtomicPtr::new(candidate));
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
    extern "C" fn poni_gc_poll_slow(&mut self) {
        let cur_flags = GC_FLAGS.load(Ordering::Relaxed);
        //eprintln!("poni-gc: poll: self = {:b}, new = {:b}", self.flag, cur_flags);
        if cur_flags == self.flag {
            // TODO: Is this the best way? it may require a few extra steps...?
            return;
        }

        self.flag = cur_flags;

        let do_scan = GC_FLAGS.load(Ordering::Relaxed) & GC_FLAG_SCAN != 0;
        if do_scan {
            self.do_scan();
        }

        let mut lock = self.shared.outstanding_threads.lock().unwrap();
        *lock -= 1;
        if *lock == 0 {
            self.shared.outstanding_thread_cv.notify_one();
        }
    }

    #[export_name = "poni_gc_poll_until_cycle_finished"]
    extern "C" fn poll_until_cycle_finished(&mut self) {
        let mut lock = self.shared.gc_flags_mutex.lock().unwrap();
        loop {
            self.poni_gc_poll_slow();
            lock = self.shared.gc_flags_cv.wait(lock).unwrap();

            let busy = self.shared.gc_busy.lock().unwrap();
            if !*busy { break; }
        }
    }
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

impl GcShared {
    pub fn new() -> Self {
        GcShared {
            queue_queue: Mutex::new(Vec::new()),
            available_threads: Mutex::new(0),
            outstanding_threads: Mutex::new(0),
            outstanding_thread_cv: Condvar::new(),
            allocator: Mutex::new(GcAllocator::new()),

            gc_request: Mutex::new(0),
            gc_request_cv: Condvar::new(),

            gc_busy: Mutex::new(false),
            gc_busy_cv: Condvar::new(),

            gc_flags_mutex: Mutex::new(()),
            gc_flags_cv: Condvar::new(),
        }
    }

    pub fn alloc(&self, size: usize) -> *mut u64 {
        let mut allocator = self.allocator.lock().unwrap();
        allocator.alloc(size)
    }
}

struct GcStatistics {
    objects_freed: u64,
    objects_kept: u64,
    bytes_freed: u64,
    bytes_kept: u64
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

        // We must always reset the first 8-bytes to 0.
        unsafe { *ptr = 0; }

        if self.allocate_marked {
            unsafe { *ptr |= 1; }
        }

        self.allocations.push(AtomicPtr::new(ptr));

        ptr
    }

    pub fn sweep(&mut self) {
        const DO_STATS: bool = false;
        let mut stats = GcStatistics {
            objects_freed: 0,
            objects_kept: 0,
            bytes_freed: 0,
            bytes_kept: 0,
        };

        // TODO: Maybe build the new_allocations array as part of marking? That
        // should save some time.
        let mut new_allocations: Vec<AtomicPtr<u64>> = vec![];
        for alloc in &self.allocations {
            let alloc = alloc.load(Ordering::Relaxed);

            // Free & skip any alloccations that aren't marked.
            if unsafe { *alloc & 1 == 0 } {
                unsafe { 
                    let size = poni_gc_get_allocation_size(alloc);
                    let layout = Layout::from_size_align(size, align_of::<u64>()).unwrap();
                    alloc::dealloc(alloc as *mut u8, layout);

                    if DO_STATS {
                        stats.objects_freed += 1;
                        stats.bytes_freed += size as u64;
                    }
                }

                // Don't add this allocation to the new_allocations list.
                continue;
            }

            // Otherwise, clear the mark bit.
            unsafe { *alloc &= !1; }
            new_allocations.push(AtomicPtr::new(alloc));

            if DO_STATS {
                stats.objects_kept += 1;
                unsafe { stats.bytes_kept += poni_gc_get_allocation_size(alloc) as u64; }
            }
        }
        self.allocations = new_allocations;

        if DO_STATS {
            eprintln!("--- gc statistics ---");
            eprintln!("objects freed: {}", stats.objects_freed);
            eprintln!("  bytes freed: {}", stats.bytes_freed);
            eprintln!("");
            eprintln!(" objects kept: {}", stats.objects_kept);
            eprintln!("   bytes kept: {}", stats.bytes_kept);
        }
    }
}

impl<'a> Gc<'a> {
    fn mark_busy(&self) {
        let mut busy = self.shared.gc_busy.lock().unwrap();
        *busy = true;
        self.shared.gc_busy_cv.notify_all();
    }

    fn mark_not_busy(&self) {
        let mut busy = self.shared.gc_busy.lock().unwrap();
        *busy = false;
        self.shared.gc_busy_cv.notify_all();
        // Also notify this cv...?
        self.shared.gc_flags_cv.notify_all();
    }

    fn handshake(&self, flags: u64) {
        let mut outstanding = self.shared.outstanding_threads.lock().unwrap();

        if *outstanding > 0 {
            panic!("poni-gc: handshake: tried to handshake while there were still outstanding threads");
        }

        *outstanding = *self.shared.available_threads.lock().unwrap();
        GC_FLAGS.fetch_update(Ordering::SeqCst, Ordering::SeqCst, 
            |f| Some(flags)).unwrap();

        // Notify any sleeping threads
        self.shared.gc_flags_cv.notify_all();

        //eprintln!("poni-gc: handshake: begin {:b} ({} threads)", flags, *outstanding);

        while *outstanding > 0 {
            outstanding = self.shared.outstanding_thread_cv.wait(outstanding).unwrap();
        }

        //eprintln!("poni-gc: handshake: finished")
    }

    pub fn collect(&mut self) {
        let start_time = Instant::now();
        {
            let mut allocator = self.shared.allocator.lock().unwrap();
            allocator.allocate_marked = true;
        }

        self.handshake(GC_FLAG_NOP);

        unsafe { poni_gc_visit_roots(self); }

        let mut toggle = GC_FLAG_DUMMY;

        loop {
            self.handshake(GC_FLAG_SCAN | toggle);
            toggle ^= GC_FLAG_DUMMY;

            // Move objects from queue_queue to regular queue
            let queue_queue: Vec<_> = {
                let mut lock = self.shared.queue_queue.lock().unwrap();
                std::mem::take(&mut lock)
            };

            for queue in queue_queue {
                for ptr in queue {
                    // Mark every pointer in the queue.
                    self.poni_gc_mark(ptr.load(Ordering::Relaxed));
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
        let end_time = Instant::now();

        //eprintln!("poni-gc: collect start-to-finish: {:?}", end_time.duration_since(start_time))
    }

    pub fn process_queue(&mut self) {
        loop {
            let Some(next) = self.mark_queue.pop_back() else {
                //eprintln!("poni-gc: mark queue emptied");
                break;
            };

            // We assume objects that are in the mark queue have already been
            // marked, and so will unconditionally be visited.
            unsafe {
                //eprintln!("poni-gc: visit: {next:?}");
                poni_gc_visit_object(self, next.load(Ordering::Relaxed));
            }
        }
    }

    /// gc_mark is the fundamental 'mark' function that must be called by
    /// poni_gc_visit_object or poni_gc_visit_roots.
    #[no_mangle]
    pub extern "C" fn poni_gc_mark(&mut self, ptr: *mut u64) {
        let is_marked = unsafe { *ptr & 1 != 0 };

        // Object already marked: Nothing to do.
        if is_marked {
            //eprintln!("mark: {ptr:?} already marked");
            return;
        }

        // Object is not marked: Mark it now, and visit it when we get a chance.
        unsafe {
            *ptr |= 1;

            //eprintln!("mark: {ptr:?} now marked: {:x}", *ptr);

            self.mark_queue.push_front(AtomicPtr::new(ptr));
        }
    }

    fn sweep(&mut self) {
        let mut allocator = self.shared.allocator.lock().unwrap();
        allocator.sweep();
        allocator.allocate_marked = false;
    }
}

impl<'a> GcHandle<'a> {
    #[export_name = "poni_gc_send_request"]
    pub fn send_request(&self, request: u64) {
        let mut req = self.shared.gc_request.lock().unwrap();
        *req |= request;
        self.shared.gc_request_cv.notify_one();
    }

    #[export_name = "poni_gc_join"]
    pub fn join(&mut self) {
        if let Some(handle) = self.join_handle.take() {
            self.send_request(GC_REQUEST_SHUTDOWN);
            handle.join().unwrap()
        }
    }

    #[export_name = "poni_gc_create_context_for_existing"]
    pub fn create_context_for_existing(&mut self) -> Box<GcContext> {
        let mut avail = self.shared.available_threads.lock().unwrap();
        *avail += 1;

        Box::new(GcContext { frame_list: null(), shared: self.shared, flag: 0 })
    }
}

#[export_name = "poni_gc_spawn"]
extern "C" fn gc_spawn() -> Box<GcHandle<'static>> {
    let shared = Box::new(GcShared::new());
    let shared = Box::leak(shared);

    let join_handle = thread::spawn(|| {
        let mut gc = Gc { mark_queue: VecDeque::new(), shared };
        loop {
            gc.mark_not_busy();

            let request = {
                let mut request = gc.shared.gc_request.lock().unwrap();
                while *request == 0 {
                    request = gc.shared.gc_request_cv.wait(request).unwrap();
                }
                let take = *request;
                *request = 0;
                take
            };

            gc.mark_busy();

            if request & GC_REQUEST_SHUTDOWN != 0 {
                break;
            }

            if request & GC_REQUEST_COLLECT != 0 {
                //eprintln!("poni-gc: start collect()");
                gc.collect();

                const AUTOCOLLECT: bool = true;
                if AUTOCOLLECT {
                    let mut request = gc.shared.gc_request.lock().unwrap();
                    *request |= GC_REQUEST_COLLECT;
                }
            }
        }

        gc.mark_not_busy();
    });

    Box::new(GcHandle {
        join_handle: Some(join_handle),
        shared
    })
}

/// Frees a single Box<GcContext> that was returned from one of the functions
/// for creating them (e.g. GcHandle::create_context_for_existing).
/// 
/// This function does nothing but drop the Box; it exists so that we can do
/// this from C.
#[export_name = "poni_gc_free_context"]
unsafe extern "C" fn free_context(context: Box<GcContext>) {
    drop(context);
}

/// Frees a Box<GcHandle> and the &GcShared reference stored inside it.
/// 
/// This function is the opposite of `gc_spawn`. It is unsafe because it is
/// freeing the shared &GcShared pointer -- if any outstanding references exist
/// to this pointer, bad things will happen.
/// 
/// As such, you should only call free_handle if you have freed every single
/// GcContext, *and* have joined the handle.
/// 
/// The main purpose of this function is to make it easier to check the GC
/// behavior with Valgrind. It allows us to clean up every allocation we make.
/// 
/// For the most part, it is not actually necessary to call these functions.
#[export_name = "poni_gc_free_handle"]
unsafe extern "C" fn free_handle(handle: Box<GcHandle>) {
    unsafe {
        let shared = Box::from_raw(handle.shared as *const GcShared as *mut GcShared);
        drop(shared);
    }
}