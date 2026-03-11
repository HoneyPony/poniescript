use std::{alloc::{self, Layout}, collections::VecDeque, marker::PhantomData, mem::MaybeUninit, ptr::null, sync::{Condvar, Mutex, atomic::{AtomicBool, AtomicPtr, AtomicU64, AtomicUsize, Ordering}}, thread::{self, JoinHandle}};

use crate::{GC_ALLOCATE_MARKED, GC_FLAGS, GcFrame, Gp, HasPsHeader, HasPsType};
// Garbage collection hooks
use crate::hot_reload::{gc_get_allocation_size, gc_visit_object, gc_visit_roots};

const GC_FLAG_SCAN: u64 = 2;
const GC_FLAG_HANDOFF_ALLOCS: u64 = 1;
// Dummy flag to toggle between so that the safepoints recognize there's work to do
const GC_FLAG_DUMMY: u64 = 4;
/// Flag indicating that the garbage collector has shut down. Necessary to safely
/// join with the thread while also collecting.
const GC_FLAG_DEAD: u64 = 8;

const GC_REQUEST_COLLECT: u64 = 1;
const GC_REQUEST_SHUTDOWN: u64 = 2;

pub struct Gc<'a> {
    mark_queue: VecDeque<AtomicPtr<u64>>,
    shared: &'a GcShared,
}

pub struct GcHandle<'a> {
    join_handle: Option<JoinHandle<()>>,
    shared: &'a GcShared,
}

struct GcAllocator {
    allocations: Vec<Vec<AtomicPtr<u64>>>,
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
    /// 
    /// IMPORTANT: When *writing* to the flags, you must hold the lock, because
    /// otherwise it is possible for a thread to hold the lock, read the flags,
    /// see they're not what they're expecting, sleep on the CV, and then never
    /// wake up, because the flags were written to *right in the middle* after
    /// we checked but before we went to sleep.
    /// 
    /// So, you must hold the lock in the following 2 cases:
    /// 1) You are writing to the flags (and want to wake up sleepers)
    /// 2) You are reading from the flags, and *deciding whether to sleep based
    ///    on this read*
    /// 
    /// It is still safe to read the flags without holding the lock, if you aren't
    /// going to go to sleep based on that decision.
    gc_flags_mutex: Mutex<()>,
    gc_flags_cv: Condvar,
}



#[repr(C)]
pub struct GcContext<'a> {
    frame_list: *const GcFrame,
    shared: &'a GcShared,
    flag: u64,
    own_allocs: Vec<AtomicPtr<u64>>,
}

impl<'a> GcContext<'a> {
    #[export_name = "poni_gc_alloc"]
    pub extern "C" fn alloc_raw_bytes(&mut self, size: usize) -> *mut u64 {
        let layout = Layout::from_size_align(size, align_of::<u64>()).unwrap();
        let ptr = unsafe { alloc::alloc(layout) };
        let ptr = ptr as *mut u64;

        // We must always reset the first 8-bytes to 0.
        unsafe { *ptr = 0; }

        if GC_ALLOCATE_MARKED.load(Ordering::Relaxed) {
            log::trace!("ctx {:?}: allocated {} bytes (marked)", self as *const _, size);
            unsafe { *ptr |= 1; }
        }
        else {
            log::trace!("ctx {:?}: allocated {} bytes (unmarked)", self as *const _, size);
        }

        self.own_allocs.push(AtomicPtr::new(ptr));

        // Interesting thing to try:
        // Always try_lock and move the allocations over.
        // This seems to in general reduce our efficiency, which makes sense; it
        // does, however, move the "safepoints" and "no safepoints" code closer
        // together in performance, which also makes sense.
        // if let Ok(mut alloc) = self.shared.allocator.try_lock() {
        //     alloc.allocations.append(&mut self.own_allocs);
        // }

        if self.own_allocs.len() >= 4096 {
            log::trace!("ctx {:?}: try to move {} allocations to gc thread", self as *const _, self.own_allocs.len());
            if let Ok(mut alloc) = self.shared.allocator.try_lock() {
                alloc.allocations.push(std::mem::take(&mut self.own_allocs));
                log::trace!("ctx {:?}: succesfully moved allocations to gc thread", self as *const _, );
            }
        }

        ptr
    }

    /// Safely (?) allocates an object of the given type, moving the given
    /// value into it.
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

    fn do_scan(&self) {
        let mut queue = Vec::new();
        let mut frame = self.frame_list;

        log::trace!("ctx {:?}: begin scan", self as *const _);

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
                        log::trace!("ctx {:?}: found mark candidate {:?}", self as *const _, candidate);
                        queue.push(AtomicPtr::new(candidate));
                    }
                }
            }

            // Walk down the list.
            frame = inner_frame.prev;
        }

        log::trace!("ctx {:?}: scan finished", self as *const _);

        let mut lock = self.shared.queue_queue.lock().unwrap();
        lock.push(queue);

        log::trace!("ctx {:?}: scan finished + pushed", self as *const _);
    }

    fn do_handoff(&mut self) {
        if self.own_allocs.is_empty() {
            log::trace!("ctx {:?}: handoff: nothing to handoff", self as *const _);
            return;
        }

        log::trace!("ctx {:?}: begin handoff", self as *const _);

        // Hand off existing allocations to the main allocator. This lets it
        // sweep independently of us doing additional allocations.
        {
            let mut allocator = self.shared.allocator.lock().unwrap();
            //let a = Instant::now();
            allocator.allocations.push(std::mem::take(&mut self.own_allocs));
            //let b = Instant::now();
            //eprintln!("do_handoff: {:?}", b.duration_since(a));
        }

        log::trace!("ctx {:?}: handoff finished", self as *const _);
        // We don't track those anymore.
        //
        // TODO: We should really have a Vec of Vectors in the allocator, so
        // that handing off our own allocs is constant-time over the number of
        // things we've allocated. This would keep mutex contention at an
        // absolute minimum (short of, e.g., assigning each vecotr a particular
        // slot so they could all hand off in parallel, which would be cool too).
        // self.own_allocs.clear();
    }

    #[unsafe(export_name = "poni_gc_poll_slow")]
    pub extern "C" fn poll_slow(&mut self) {
        let cur_flags = GC_FLAGS.load(Ordering::Relaxed);
        //eprintln!("poni-gc: poll: self = {:b}, new = {:b}", self.flag, cur_flags);
        if cur_flags == self.flag {
            // TODO: Is this the best way? it may require a few extra steps...?
            return;
        }

        self.flag = cur_flags;

        let do_scan = cur_flags & GC_FLAG_SCAN != 0;
        if do_scan {
            self.do_scan();
        }

        let do_handoff = cur_flags & GC_FLAG_HANDOFF_ALLOCS != 0;
        if do_handoff {
            self.do_handoff();
        }

        let mut lock = self.shared.outstanding_threads.lock().unwrap();
        *lock -= 1;
        if *lock == 0 {
            self.shared.outstanding_thread_cv.notify_all();
        }
    }

    #[export_name = "poni_gc_poll_until_cycle_finished"]
    extern "C" fn poll_until_cycle_finished(&mut self) {
        let mut lock = self.shared.gc_flags_mutex.lock().unwrap();

        // First, we have to wait for the first request from the GC (the GC should
        // be busy). This is because otherwise, the following sequence doesn't work:
        //
        // send_request(SHUTDOWN);
        // poll_until_cycle_finished();
        // join_gc();
        //
        // This is because, we might immediately finish our poll, because the
        // garbage collector isn't busy (becasue it hasn't seen our request yet).
        // This leaves the GC spinning forever in handshake waiting for us to 
        // finish, which we won't, because we're waiting on the GC to join
        // (i.e. it is a good ol' deadlock).
        //
        // So instead, we want to wait until gc_busy is true.

        {
            let mut busy = self.shared.gc_busy.lock().unwrap();
            loop {
                if *busy { break; }

                busy = self.shared.gc_busy_cv.wait(busy).unwrap();
            }
        }

        loop {
            self.poll_slow();
            lock = self.shared.gc_flags_cv.wait(lock).unwrap();

            let busy = self.shared.gc_busy.lock().unwrap();
            if !*busy { break; }
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
        }
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

        for alloc_set in &self.allocations {
            for alloc in alloc_set { 
                let alloc = alloc.load(Ordering::Relaxed);

                // Free & skip any alloccations that aren't marked.
                if unsafe { *alloc & 1 == 0 } {
                    unsafe { 
                        let size = gc_get_allocation_size(alloc);
                        let layout = Layout::from_size_align(size, align_of::<u64>()).unwrap();
                        log::trace!("poni-gc: freeing {:?} ({} bytes, tag {:x})", alloc, size, *alloc);
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
                    unsafe { stats.bytes_kept += gc_get_allocation_size(alloc) as u64; }
                }
            }
        }
        self.allocations.clear();
        self.allocations.push(new_allocations);

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

        {
            // Must hold the lock while writing the flags, as per the comment
            // above.
            let _lock = self.shared.gc_flags_mutex.lock().unwrap();
            GC_FLAGS.store(flags, Ordering::Relaxed);
            //GC_FLAGS.fetch_update(Ordering::SeqCst, Ordering::SeqCst, 
            //    |f| Some(flags)).unwrap();   
         }
        

        // Notify any sleeping threads
        self.shared.gc_flags_cv.notify_all();

        log::trace!("poni-gc: handshake: begin {:b} ({} threads)", flags, *outstanding);

        while *outstanding > 0 {
            outstanding = self.shared.outstanding_thread_cv.wait(outstanding).unwrap();
        }

        log::trace!("poni-gc: handshake: finished")
    }

    pub fn collect(&mut self) {
        log::info!("begin collection cycle");
        GC_ALLOCATE_MARKED.store(true, Ordering::Relaxed);

        self.handshake(GC_FLAG_HANDOFF_ALLOCS);

        unsafe { gc_visit_roots(self); }

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

        // We've marked everything. Because new allocations do not go into
        // the global allocator anymore, there's no need to keep allocating
        // thing as marked.
        //
        // We still do this relaxed for the time begin, but we might want
        // to consider making it stronger to ensure threads start allocating
        // un-marked ASAP (?)
        GC_ALLOCATE_MARKED.store(false, Ordering::Relaxed);

        self.sweep();
    }

    pub fn process_queue(&mut self) {
        loop {
            let Some(next) = self.mark_queue.pop_back() else {
                //eprintln!("poni-gc: mark queue emptied");
                break;
            };

            unsafe { gc_visit_object(self, next.load(Ordering::Relaxed)); }
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
        log::info!("beginning sweep");
        allocator.sweep();
        log::info!("sweep finished");
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
    pub fn join(&mut self, context: Option<&mut GcContext>) {
        if let Some(handle) = self.join_handle.take() {
            self.send_request(GC_REQUEST_SHUTDOWN);
            
            if let Some(context) = context {
                let mut lock = context.shared.gc_flags_mutex.lock().unwrap();
                loop {
                    // Wait until the garbage collector is dead.
                    if GC_FLAGS.load(Ordering::Relaxed) & GC_FLAG_DEAD != 0 {
                        break;
                    }

                    // Otherwise, perform a safepoint. We might as well do it
                    // the slow way because we're just spinning anyway.
                    context.poll_slow();

                    lock = context.shared.gc_flags_cv.wait(lock).unwrap();
                }
            }

            handle.join().unwrap()
        }
    }

    #[export_name = "poni_gc_create_context_for_existing"]
    pub fn create_context_for_existing(&mut self) -> Box<GcContext<'_>> {
        let mut avail = self.shared.available_threads.lock().expect("available threads lock");
        *avail += 1;

        Box::new(GcContext { frame_list: null(), shared: self.shared, flag: 0, own_allocs: Vec::new() })
    }
}

#[export_name = "poni_gc_spawn"]
pub extern "C" fn gc_spawn() -> Box<GcHandle<'static>> {
    // TODO: This should not be in the GC crate at all, but it is convenient
    // for the time being.
    // env_logger::init();

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

            // COLLECT request takes priority over SHUTDOWN request, so that
            // we can force a collection at shutdown time.
            if request & GC_REQUEST_COLLECT != 0 {
                //eprintln!("poni-gc: start collect()");
                gc.collect();

                const AUTOCOLLECT: bool = false;
                if AUTOCOLLECT {
                    let mut request = gc.shared.gc_request.lock().unwrap();
                    *request |= GC_REQUEST_COLLECT;
                }
            }

            if request & GC_REQUEST_SHUTDOWN != 0 {
                break;
            }
        }

        gc.mark_not_busy();

        {
            // Must hold the lock when writing a value, as per the documentation.
            let _lock = gc.shared.gc_flags_mutex.lock();
            GC_FLAGS.store(GC_FLAG_DEAD, Ordering::Relaxed);
        }
        gc.shared.gc_flags_cv.notify_all();
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