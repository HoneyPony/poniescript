use std::{cell::UnsafeCell, marker::PhantomData, num::{NonZeroU32, NonZeroUsize}, ops::{Deref, DerefMut}};

pub trait ArenaKey: Copy {
    fn to_nonzero_usize(self) -> NonZeroUsize;
    unsafe fn from_nonzero_u32(id: NonZeroU32) -> Self;

    #[inline(always)]
    fn to_index(self) -> usize {
        self.to_nonzero_usize().get() - 1
    }

    #[inline(always)]
    unsafe fn from_index(index: usize) -> Self {
        #[cfg(debug_assertions)]
        {
            Self::from_nonzero_u32(NonZeroU32::new(index as u32 + 1).unwrap())
        }

        #[cfg(not(debug_assertions))]
        {
            Self::from_nonzero_u32(NonZeroU32::new_unchecked(index as u32 + 1))
        }
    }

    #[inline(always)]
    unsafe fn invalid() -> Self {
        Self::from_index(0)
    }
}

macro_rules! define_arena_key {
    ($key_name:ident) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $key_name(std::num::NonZeroU32);

        // TODO: Is this the best fully qualified name for this trait?
        impl crate::arena::ArenaKey for $key_name {
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
            objects: Vec::new(),
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
    fn get(&self, key: Key) -> ArenaBorrow<Ty, Key>;
    fn get_mut(&self, key: Key) -> ArenaBorrowMut<Ty, Key>;
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
            objects: UnsafeCell::new(Vec::new()),

            phantom: PhantomData{},

            #[cfg(debug_assertions)]
            borrowed_mut: UnsafeCell::new(Vec::new()),

            #[cfg(debug_assertions)]
            borrowed: UnsafeCell::new(Vec::new()),
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

    pub fn get_proxy(&mut self) -> ArenaCellProxy<Ty, Key> {
        return ArenaCellProxy {
            arena: self, added: UnsafeCell::new(Vec::new()),

            #[cfg(debug_assertions)]
            borrowed_mut: UnsafeCell::new(Vec::new()),

            #[cfg(debug_assertions)]
            borrowed: UnsafeCell::new(Vec::new()),
        }
    }

    // TODO:
    // Rust appears to get angry if we do the following:
    //
    //    let p = a.get_proxy();
    //    ...
    //    a.commit_proxy(p);
    //
    // In the case that commit_proxy takes an &mut self. This is because the
    // commit_proxy counts as a second borrow, even though the whole point
    // is that we're consuming the old borrow. So for now, we just do it
    // a stupid way where we take &self even though that is NOT safe.
    pub fn commit_proxy(&self, proxy: ArenaCellProxy<Ty, Key>) {
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
}

impl<Ty, Key: ArenaKey> IndexCell<Ty, Key> for ArenaCell<Ty, Key> {
    fn get(&self, id: Key) -> ArenaBorrow<Ty, Key> {
        let idx = id.to_index();

        #[cfg(debug_assertions)]
        unsafe {
            let mut_borrows = self.borrowed_mut.get().as_mut().unwrap();
            if mut_borrows[idx] {
                panic!("ArenaCell: double-borrow for {idx} (immutable borrow during mutable borrow)");
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

    fn get_mut(&self, id: Key) -> ArenaBorrowMut<Ty, Key> {
        let idx = id.to_index();

        #[cfg(debug_assertions)]
        unsafe {
            let mut_borrows = self.borrowed_mut.get().as_mut().unwrap();
            if mut_borrows[idx] {
                panic!("ArenaCell: double-borrow for {idx} (second mutable borrow)");
            }

            let counters = self.borrowed.get().as_mut().unwrap();
            if counters[idx] > 0 {
                panic!("ArenaCell: attempt to mutably borrow {idx} while it is immutable borrowed ({} times)", counters[idx]);
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
        self.arena.commit_proxy(self);
    }

    pub fn push(&self, object: Ty) -> Key {
        unsafe {
            (*self.added.get()).push(Box::new(object));
        
            #[cfg(debug_assertions)]
            (*self.borrowed_mut.get()).push(false);

            Key::from_index(self.parent_len() + (*self.added.get()).len() - 1)
        }
    }
}

impl<'a, Ty, Key: ArenaKey> IndexCell<Ty, Key> for ArenaCellProxy<'a, Ty, Key> {
    fn get(&self, id: Key) -> ArenaBorrow<Ty, Key> {
        let idx = id.to_index();

        if idx < self.parent_len() {
            return self.arena.get(id);
        }

        let idx = idx - self.parent_len();

        #[cfg(debug_assertions)]
        unsafe {
            let mut_borrows = self.borrowed_mut.get().as_mut().unwrap();
            if mut_borrows[idx] {
                panic!("ArenaCell: double-borrow for {idx} (immutable borrow during mutable borrow)");
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

    fn get_mut(&self, id: Key) -> ArenaBorrowMut<Ty, Key> {
        let idx = id.to_index();

        if idx < self.parent_len() {
            return self.arena.get_mut(id);
        }

        let idx = idx - self.parent_len();

        #[cfg(debug_assertions)]
        unsafe {
            let mut_borrows = self.borrowed_mut.get().as_mut().unwrap();
            if mut_borrows[idx] {
                panic!("ArenaCell: double-borrow for {idx} (second mutable borrow)");
            }

            let counters = self.borrowed.get().as_mut().unwrap();
            if counters[idx] > 0 {
                panic!("ArenaCell: attempt to mutably borrow {idx} while it is immutable borrowed ({} times)", counters[idx]);
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