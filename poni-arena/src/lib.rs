use std::{cell::UnsafeCell, marker::PhantomData, num::{NonZeroU32, NonZeroUsize}, ops::{Deref, DerefMut}};

const DEFAULT_CAPACITY: usize = 256;

/// Trait for keys for Arenas.
/// 
/// These are the core methods necessary to use a particular opaque type as
/// a key for the various Arena types. However, for the most part you do not need
/// to implement ArenaKey manually. Instead, use [`define_arena_key!`] for defining
/// your opaque key types.
pub trait ArenaKey: Copy {
    fn to_nonzero_usize(self) -> NonZeroUsize;
    unsafe fn from_nonzero_u32(id: NonZeroU32) -> Self;

    #[inline(always)]
    fn to_index(self) -> usize {
        self.to_nonzero_usize().get() - 1
    }

    #[inline(always)]
    unsafe fn from_index(index: usize) -> Self {
        unsafe {
            #[cfg(debug_assertions)]
            {
                Self::from_nonzero_u32(NonZeroU32::new(index as u32 + 1).unwrap())
            }

            #[cfg(not(debug_assertions))]
            {
                Self::from_nonzero_u32(NonZeroU32::new_unchecked(index as u32 + 1))
            }
        }
    }

    #[inline(always)]
    unsafe fn invalid() -> Self {
        unsafe { Self::from_index(0) }
    }
}

/// Defines a new key type for an Arena.
/// 
/// The idea here is to associate a new opaque type with each kind of Arena.
/// For example, if you have an Arena of Exprs and an Arena of Stmts, you may
/// want an ExprKey/ExprId and a StmtKey/StmtId.
/// 
/// In this case, you could `define_arena_key!(ExprId);` and then later construct
/// a new arena such as `let arena: Arena<Expr, ExprId> = ...;`
/// 
/// Note that Arena keys are currently not associated in any way with the Arena
/// instance they belong to. This may change in the future.
#[macro_export]
macro_rules! define_arena_key {
    ($key_name:ident) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $key_name(std::num::NonZeroU32);

        // TODO: Is this the best fully qualified name for this trait?
        impl $crate::ArenaKey for $key_name {
            #[inline(always)]
            fn to_nonzero_usize(self) -> std::num::NonZeroUsize {
                // Safety: Our self.0 is always nonzero, so this should always also
                // return nonzero.
                unsafe { std::num::NonZeroUsize::new_unchecked(self.0.get() as usize) }
            }

            #[inline(always)]
            unsafe fn from_nonzero_u32(id: std::num::NonZeroU32) -> Self {
                $key_name(id)
            }
        }
    }
}

/// The simplest Arena type.
/// 
/// This Arena is essentially a `Vec<Ty>`, indexed by the `Key` type. The main
/// value it provides is to enable the Key types to be opaque, rather than 
/// normal `usize`s, and to enable conversion into an ArenaCell.
pub struct Arena<Ty, Key: ArenaKey> {
    objects: Vec<Ty>,
    phantom: PhantomData<Key>
}

// Note: Previously we also had a phantom lifetime parameter, but that is more
// annoying than it is worth.
pub struct ArenaIterator<Ty, Key: ArenaKey> {
    len: usize,
    current: usize,

    phantom: PhantomData<(Ty, Key)>,
}

impl<Ty, Key: ArenaKey> std::iter::Iterator for ArenaIterator<Ty, Key> {
    type Item = Key;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current == self.len {
            return None;
        }

        let result = unsafe { Some(Key::from_index(self.current)) };

        self.current += 1;

        result
    }
}

impl<Ty, Key: ArenaKey> Arena<Ty, Key> {
    pub fn new() -> Self {
        Arena {
            objects: Vec::with_capacity(DEFAULT_CAPACITY),
            phantom: PhantomData{}
        }
    }

    pub fn iter(&self) -> ArenaIterator<Ty, Key> {
        ArenaIterator {
            len: self.objects.len(),
            current: 0,
            phantom: PhantomData{}
        }
    }

    pub fn push(&mut self, object: Ty) -> Key {
        self.objects.push(object);
        unsafe { Key::from_index(self.objects.len() - 1) }
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn into_cell(self) -> ArenaCell<Ty, Key> {
        // TODO: Make this less inefficient? Oh well.
        let mut cell: ArenaCell<Ty, Key> = ArenaCell::new();
        for obj in self.objects.into_iter() {
            cell.push(obj);
        }
        cell
    }

    #[cfg(debug_assertions)]
    pub fn get(&self, key: Key) -> &Ty {
        self.objects.get(key.to_index()).unwrap()
    }

    #[cfg(not(debug_assertions))]
    pub fn get(&self, key: Key) -> &Ty {
        unsafe { self.objects.get_unchecked(key.to_index()) }
    }

    #[cfg(debug_assertions)]
    pub fn get_mut(&mut self, key: Key) -> &mut Ty {
        self.objects.get_mut(key.to_index()).unwrap()
    }

    #[cfg(not(debug_assertions))]
    pub fn get_mut(&mut self, key: Key) -> &mut Ty {
        unsafe { self.objects.get_unchecked_mut(key.to_index()) }
    }
}

/// This type is essentially a Vec<RefCell<T>>, except that the borrow checking
/// for the inner T happens entirely in a side table, rather than per-item.
/// 
/// There is also an additional caveat that, as of this writing, ArenaCell
/// is *intentionally* unsound. In particular, the runtime borrow-checking rules
/// (a la RefCell) are only enabled when `debug_assertions` are enabled. In
/// release mode, the borrow checking rules are entirely elided.
/// 
/// The motivation for this is that the target use-case of ArenaCell, namely
/// compiler data structures, does not really enable 'accidentally' forgetting
/// to drop a borrow. If you have a half-decent test suite, and you have an
/// accidental double-borrow, you WILL see the panic in debug mode, and you
/// will be able to trivially fix it.
/// 
/// In the future, there may be some 
pub struct ArenaCell<Ty, Key: ArenaKey> {
    objects: UnsafeCell<Vec<Ty>>,

    phantom: PhantomData<Key>,

    #[cfg(debug_assertions)]
    borrowed_mut: UnsafeCell<Vec<bool>>,

    #[cfg(debug_assertions)]
    borrowed: UnsafeCell<Vec<usize>>,
}

pub struct ArenaCellProxy<'ar, Ty, Key: ArenaKey> {
    arena: &'ar ArenaCell<Ty, Key>,

    added: UnsafeCell<Vec<Box<Ty>>>,

    #[cfg(debug_assertions)]
    borrowed_mut: UnsafeCell<Vec<bool>>,

    #[cfg(debug_assertions)]
    borrowed: UnsafeCell<Vec<usize>>,
}

pub trait IndexCell<Ty, Key: ArenaKey> {
    fn get(&self, key: Key) -> ArenaBorrow<'_, Ty, Key>;
    fn get_mut(&self, key: Key) -> ArenaBorrowMut<'_, Ty, Key>;
}

enum ArenaBorrowParent<'a, Ty, Key: ArenaKey> {
    Cell(&'a ArenaCell<Ty, Key>),
    Proxy(&'a ArenaCellProxy<'a, Ty, Key>)
}

pub struct ArenaBorrowMut<'a, Ty, Key: ArenaKey> {
    inner: &'a mut Ty,

    #[cfg(debug_assertions)]
    parent: ArenaBorrowParent<'a, Ty, Key>,
    #[cfg(debug_assertions)]
    idx: Key,

    #[cfg(not(debug_assertions))]
    phantom: PhantomData<Key>,
}

pub struct ArenaBorrow<'a, Ty, Key: ArenaKey> {
    inner: &'a Ty,

    #[cfg(debug_assertions)]
    parent: ArenaBorrowParent<'a, Ty, Key>,
    #[cfg(debug_assertions)]
    idx: Key,

    #[cfg(not(debug_assertions))]
    phantom: PhantomData<Key>,
}

impl<'a, Ty, Key: ArenaKey> AsRef<Ty> for ArenaBorrowMut<'a, Ty, Key> {
    fn as_ref(&self) -> &Ty {
        self.inner
    }
}

impl<'a, Ty, Key: ArenaKey> AsMut<Ty> for ArenaBorrowMut<'a, Ty, Key> {
    fn as_mut(&mut self) -> &mut Ty {
        self.inner
    }
}

impl<'a, Ty, Key: ArenaKey> Deref for ArenaBorrowMut<'a, Ty, Key> {
    type Target = Ty;

    fn deref(&self) -> &Self::Target {
        self.inner
    }
}

impl<'a, Ty, Key: ArenaKey> DerefMut for ArenaBorrowMut<'a, Ty, Key> {    
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner
    }
}

impl<'a, Ty, Key: ArenaKey> AsRef<Ty> for ArenaBorrow<'a, Ty, Key> {
    fn as_ref(&self) -> &Ty {
        self.inner
    }
}

impl<'a, Ty, Key: ArenaKey> Deref for ArenaBorrow<'a, Ty, Key> {
    type Target = Ty;

    fn deref(&self) -> &Self::Target {
        self.inner
    }
}

impl<'a, Ty, Key: ArenaKey> Drop for ArenaBorrowMut<'a, Ty, Key> {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        unsafe {
            match self.parent {
                ArenaBorrowParent::Cell(arena_cell) => {
                    let mut_borrows = arena_cell.borrowed_mut.get().as_mut().unwrap();
                    mut_borrows[self.idx.to_index()] = false;
                }
                ArenaBorrowParent::Proxy(arena_cell_proxy) => {
                    let idx = self.idx.to_index();
                    if idx < arena_cell_proxy.parent_len() {
                        let mut_borrows = arena_cell_proxy.arena.borrowed_mut.get().as_mut().unwrap();
                        mut_borrows[idx] = false;
                    }
                    else {
                        let idx = idx - arena_cell_proxy.parent_len();
                        let mut_borrows = arena_cell_proxy.borrowed_mut.get().as_mut().unwrap();
                        mut_borrows[idx] = false;
                    }
                }
            }
            
        }
    }
}

impl<'a, Ty, Key: ArenaKey> Drop for ArenaBorrow<'a, Ty, Key> {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        unsafe {
            match self.parent {
                ArenaBorrowParent::Cell(arena_cell) => {
                    let counters = arena_cell.borrowed.get().as_mut().unwrap();
                    counters[self.idx.to_index()] -= 1;
                }
                ArenaBorrowParent::Proxy(arena_cell_proxy) => {
                    let idx = self.idx.to_index();
                    if idx < arena_cell_proxy.parent_len() {
                        let counters = arena_cell_proxy.arena.borrowed.get().as_mut().unwrap();
                        counters[idx] -= 1;
                    }
                    else {
                        let idx = idx - arena_cell_proxy.parent_len();
                        let counters = arena_cell_proxy.borrowed.get().as_mut().unwrap();
                        counters[idx] -= 1;
                    }
                }
            }
        }
    }
}


impl<Ty, Key: ArenaKey> ArenaCell<Ty, Key> {
    pub fn new() -> Self {
        ArenaCell {
            objects: UnsafeCell::new(Vec::with_capacity(DEFAULT_CAPACITY)),

            phantom: PhantomData{},

            #[cfg(debug_assertions)]
            borrowed_mut: UnsafeCell::new(Vec::with_capacity(DEFAULT_CAPACITY)),

            #[cfg(debug_assertions)]
            borrowed: UnsafeCell::new(Vec::with_capacity(DEFAULT_CAPACITY)),
        }
    }

    pub fn into_readonly(self) -> Arena<Ty, Key> {
        Arena {
            objects: self.objects.into_inner(),
            phantom: PhantomData,
        }
    }

    pub fn push(&mut self, object: Ty) -> Key {
        self.objects.get_mut().push(object);
        
        #[cfg(debug_assertions)]
        self.borrowed_mut.get_mut().push(false);

        #[cfg(debug_assertions)]
        self.borrowed.get_mut().push(0);

        unsafe { Key::from_index(self.objects.get_mut().len() - 1) }
    }

    pub fn get_proxy(&mut self) -> ArenaCellProxy<'_, Ty, Key> {
        return ArenaCellProxy {
            arena: self, added: UnsafeCell::new(Vec::new()),

            #[cfg(debug_assertions)]
            borrowed_mut: UnsafeCell::new(Vec::new()),

            #[cfg(debug_assertions)]
            borrowed: UnsafeCell::new(Vec::new()),
        }
    }

    // Rust appears to get angry if we do the following:
    //
    //    let p = a.get_proxy();
    //    ...
    //    a.commit_proxy(p);
    //
    // In the case that commit_proxy takes an &mut self. This is because the
    // commit_proxy counts as a second borrow, even though the whole point
    // is that we're consuming the old borrow.
    //
    // So now, we make the proxy responsible for committing itself, and make
    // it clear that this is unsafe.
    unsafe fn commit_proxy(&self, proxy: ArenaCellProxy<Ty, Key>) {
        // TODO:
        // Do the items in the Proxy need to be Pinned? Maybe????
        let items: Vec<_> = proxy.added.into_inner();
        unsafe {
            for item in items {
                self.objects.get().as_mut().unwrap().push(*item);

                // TODO: Do we need to copy the borrowed value from the Proxy?
                #[cfg(debug_assertions)]
                self.borrowed_mut.get().as_mut().unwrap().push(false);

                #[cfg(debug_assertions)]
                self.borrowed.get().as_mut().unwrap().push(0);
            }
        }
    }

    pub fn iter(&self) -> ArenaCellIterator<'_, Ty, Key> {
        ArenaCellIterator { current: 0, arena: self }
    }
}

impl<Ty, Key: ArenaKey> IndexCell<Ty, Key> for ArenaCell<Ty, Key> {
    fn get(&self, id: Key) -> ArenaBorrow<'_, Ty, Key> {
        let idx = id.to_index();

        #[cfg(debug_assertions)]
        unsafe {
            let mut_borrows = self.borrowed_mut.get().as_mut().unwrap();
            if mut_borrows[idx] {
                panic!("ICE: ArenaCell: double-borrow for {idx} (immutable borrow during mutable borrow)");
            }

            let counters = self.borrowed.get().as_mut().unwrap();
            counters[idx] += 1;
        }

        #[cfg(debug_assertions)]
        let ptr = unsafe { self.objects.get().as_mut().unwrap().get(idx).unwrap() };

        #[cfg(not(debug_assertions))]
        let ptr = unsafe { self.objects.get().as_mut().unwrap_unchecked().get_unchecked(idx) };

        return ArenaBorrow {
            inner: ptr,
            #[cfg(debug_assertions)]
            parent: ArenaBorrowParent::Cell(self),
            #[cfg(debug_assertions)]
            idx: id,

            #[cfg(not(debug_assertions))]
            phantom: PhantomData{},
        };
    }

    fn get_mut(&self, id: Key) -> ArenaBorrowMut<'_, Ty, Key> {
        let idx = id.to_index();

        #[cfg(debug_assertions)]
        unsafe {
            let mut_borrows = self.borrowed_mut.get().as_mut().unwrap();
            if mut_borrows[idx] {
                panic!("ICE: ArenaCell: double-borrow for {idx} (second mutable borrow)");
            }

            let counters = self.borrowed.get().as_mut().unwrap();
            if counters[idx] > 0 {
                panic!("ICE: ArenaCell: attempt to mutably borrow {idx} while it is immutable borrowed ({} times)", counters[idx]);
            }

            mut_borrows[idx] = true;
        }

        // This nonsense makes Miri happy
        #[cfg(debug_assertions)]
        let ptr = unsafe { self.objects.get().as_mut().unwrap().as_mut_ptr().add(idx).as_mut().unwrap() };

        #[cfg(not(debug_assertions))]
        let ptr = unsafe { self.objects.get().as_mut().unwrap_unchecked().as_mut_ptr().add(idx).as_mut().unwrap_unchecked() };

        // Perhaps the more natural expression:
        // let ptr = unsafe { self.exprs.get().as_mut().unwrap().get_mut(id).unwrap() };

        return ArenaBorrowMut {
            inner: ptr,
            #[cfg(debug_assertions)]
            parent: ArenaBorrowParent::Cell(self),
            #[cfg(debug_assertions)]
            idx: id,

            #[cfg(not(debug_assertions))]
            phantom: PhantomData{},
        };
    }
}

impl <'ar, Ty, Key: ArenaKey> ArenaCellProxy<'ar, Ty, Key> {
    fn parent_len(&self) -> usize {
        unsafe { (*self.arena.objects.get()).len() }
    }

    pub fn commit(self) {
        // Safety: This consumes ourselves as the proxy, while our parent is
        // currently mutably borrowed by us (get_proxy takes a &mut Arena).
        //
        // So, we are the only possible proxy, meaning it is safe for us to
        // commit, and now that we are consumed, the parent arena can be
        // borrowed again.
        unsafe { self.arena.commit_proxy(self); }
    }

    pub fn push(&self, object: Ty) -> Key {
        unsafe {
            (*self.added.get()).push(Box::new(object));
        
            #[cfg(debug_assertions)]
            (*self.borrowed_mut.get()).push(false);
            #[cfg(debug_assertions)]
            (*self.borrowed.get()).push(0);

            Key::from_index(self.parent_len() + (*self.added.get()).len() - 1)
        }
    }

    pub fn iter<'p>(&'p self) -> ArenaCellProxyIterator<'p, 'ar, Ty, Key> {
        ArenaCellProxyIterator { current: 0, arena: self }
    }
}

impl<'a, Ty, Key: ArenaKey> IndexCell<Ty, Key> for ArenaCellProxy<'a, Ty, Key> {
    fn get(&self, id: Key) -> ArenaBorrow<'_, Ty, Key> {
        let idx = id.to_index();

        if idx < self.parent_len() {
            return self.arena.get(id);
        }

        let idx = idx - self.parent_len();

        #[cfg(debug_assertions)]
        unsafe {
            let mut_borrows = self.borrowed_mut.get().as_mut().unwrap();
            if mut_borrows[idx] {
                panic!("ICE: ArenaCell: double-borrow for {idx} (immutable borrow during mutable borrow)");
            }

            let counters = self.borrowed.get().as_mut().unwrap();
            counters[idx] += 1;
        }

        #[cfg(debug_assertions)]
        let ptr = unsafe { self.added.get().as_mut().unwrap().get(idx).unwrap().as_ref() };

        #[cfg(not(debug_assertions))]
        let ptr = unsafe { self.added.get().as_mut().unwrap_unchecked().get_unchecked(idx).as_ref() };

        return ArenaBorrow {
            inner: ptr,
            #[cfg(debug_assertions)]
            parent: ArenaBorrowParent::Proxy(self),
            #[cfg(debug_assertions)]
            idx: id,

            #[cfg(not(debug_assertions))]
            phantom: PhantomData{},
        };
    }

    fn get_mut(&self, id: Key) -> ArenaBorrowMut<'_, Ty, Key> {
        let idx = id.to_index();

        if idx < self.parent_len() {
            return self.arena.get_mut(id);
        }

        let idx = idx - self.parent_len();

        #[cfg(debug_assertions)]
        unsafe {
            let mut_borrows = self.borrowed_mut.get().as_mut().unwrap();
            if mut_borrows[idx] {
                panic!("ICE: ArenaCell: double-borrow for {idx} (second mutable borrow)");
            }

            let counters = self.borrowed.get().as_mut().unwrap();
            if counters[idx] > 0 {
                panic!("ICE: ArenaCell: attempt to mutably borrow {idx} while it is immutable borrowed ({} times)", counters[idx]);
            }

            mut_borrows[idx] = true;
        }

        // This nonsense makes Miri happy
        #[cfg(debug_assertions)]
        let ptr = unsafe { self.added.get().as_mut().unwrap().as_mut_ptr().add(idx).as_mut().unwrap().as_mut() };

        #[cfg(not(debug_assertions))]
        let ptr = unsafe { self.added.get().as_mut().unwrap_unchecked().as_mut_ptr().add(idx).as_mut().unwrap_unchecked().as_mut() };

        // Perhaps the more natural expression:
        // let ptr = unsafe { self.exprs.get().as_mut().unwrap().get_mut(id).unwrap() };

        return ArenaBorrowMut {
            inner: ptr,
            #[cfg(debug_assertions)]
            parent: ArenaBorrowParent::Proxy(self),
            #[cfg(debug_assertions)]
            idx: id,

            #[cfg(not(debug_assertions))]
            phantom: PhantomData{},
        };
    }
}

pub struct ArenaCellIterator<'ar, Ty, Key: ArenaKey> {
    current: usize,
    arena: &'ar ArenaCell<Ty, Key>
}

pub struct ArenaCellProxyIterator<'p, 'ar, Ty, Key: ArenaKey> {
    current: usize,
    arena: &'p ArenaCellProxy<'ar, Ty, Key>
}

impl <'ar, Ty, Key: ArenaKey>  Iterator for ArenaCellIterator<'ar, Ty, Key> {
    type Item = Key;

    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            let cur_idx = self.current;
            if cur_idx >= self.arena.objects.get().as_ref().unwrap().len() { return None; }

            self.current += 1;
            return Some(Key::from_index(cur_idx))
        }
    }
}

impl<'p, 'ar, Ty, Key: ArenaKey> Iterator for ArenaCellProxyIterator<'p, 'ar, Ty, Key> {
    type Item = Key;

    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            let cur_idx = self.current;
            if cur_idx >= self.arena.parent_len() + self.arena.added.get().as_ref().unwrap().len() { return None; }

            self.current += 1;
            return Some(Key::from_index(cur_idx))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    define_arena_key!(TestId);

    #[test]
    fn test_correct() {
        let mut arena = ArenaCell::<i32, TestId>::new();

        let mut keys: Vec<TestId> = vec![];

        for _ in 0..5 {
            keys.push(arena.push(10));
        }

        fn update_all(arena: &ArenaCell<i32, TestId>, keys: &Vec<TestId>) {
            for key in keys {
                *arena.get_mut(*key).as_mut() += 10;
            }
        }

        update_all(&arena, &keys);
        update_all(&arena, &keys);

        for key in keys {
            assert_eq!(*arena.get(key), 30);
        }
    }

    #[test]
    fn test_reader_writer() {
        let mut arena = ArenaCell::<i32, TestId>::new();

        let mut keys: Vec<TestId> = vec![];

        for _ in 0..5 {
            keys.push(arena.push(10));
        }

        fn grab_objects<'a>(arena: &'a ArenaCell<i32, TestId>, keys: &Vec<TestId>) -> Vec<ArenaBorrow<'a, i32, TestId>> {
            let mut borrows = vec![];
            for key in keys {
                borrows.push(arena.get(*key));
            }
            borrows
        }

        let b1 = grab_objects(&arena, &keys);
        let b2 = grab_objects(&arena, &keys);
        let b3 = grab_objects(&arena, &keys);

        drop(b1);
        drop(b2);
        drop(b3);

        fn update_all(arena: &ArenaCell<i32, TestId>, keys: &Vec<TestId>) {
            for key in keys {
                *arena.get_mut(*key).as_mut() += 10;
            }
        }

        update_all(&arena, &keys);
        update_all(&arena, &keys);

        for key in keys {
            assert_eq!(*arena.get(key), 30);
        }
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "ArenaCell: double-borrow for 0 (second mutable borrow)")]
    fn test_failure_mut() {
        let mut arena = ArenaCell::<i32, TestId>::new();
        let a = arena.push(5);
        arena.push(10);

        let a_borrow = arena.get_mut(a);
        // should panic
        let a_borrow2 = arena.get_mut(a);

        eprintln!("{} {}", *a_borrow, *a_borrow2);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "ArenaCell: double-borrow for 0 (immutable borrow during mutable borrow)")]
    fn test_failure_immut() {
        let mut arena = ArenaCell::<i32, TestId>::new();
        let a = arena.push(5);
        arena.push(10);

        let a_borrow = arena.get_mut(a);
        // should panic
        let a_borrow2 = arena.get(a);

        eprintln!("{} {}", *a_borrow, *a_borrow2);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "ArenaCell: attempt to mutably borrow 0 while it is immutable borrowed (3 times)")]
    fn test_failure_mut_during_unmut() {
        let mut arena = ArenaCell::<i32, TestId>::new();
        let a = arena.push(5);

        let _b1 = arena.get(a);
        let _b2 = arena.get(a);
        let _b3 = arena.get(a);

        // Should panic
        let _mut_b = arena.get_mut(a);
    }
}