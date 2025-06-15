use std::{cell::UnsafeCell, marker::PhantomData, num::{NonZeroU32, NonZeroUsize}, ops::{Deref, DerefMut}};

trait ArenaKey: Copy {
    fn to_usize(self) -> NonZeroUsize;
    unsafe fn from_u32(id: NonZeroU32) -> Self;

    fn to_index(self) -> usize {
        self.to_usize().get() - 1
    }

    unsafe fn from_index(index: usize) -> Self {
        #[cfg(debug_assertions)]
        {
            Self::from_u32(NonZeroU32::new(index as u32 + 1).unwrap())
        }

        #[cfg(not(debug_assertions))]
        {
            Self::from_u32(NonZeroU32::new_unchecked(index as u32 + 1))
        }
    }
}

macro_rules! define_arena_key {
    ($key_name:ident) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash)]
        struct $key_name(NonZeroU32);

        impl ArenaKey for $key_name {
            fn to_usize(self) -> NonZeroUsize {
                // Safety: Our self.0 is always nonzero, so this should always also
                // return nonzero.
                unsafe { NonZeroUsize::new_unchecked(self.0.get() as usize) }
            }

            unsafe fn from_u32(id: NonZeroU32) -> Self {
                $key_name(id)
            }
        }
    }
}
struct ArenaCell<Ty, Key: ArenaKey> {
    objects: UnsafeCell<Vec<Ty>>,

    phantom: PhantomData<Key>,

    #[cfg(debug_assertions)]
    borrowed: UnsafeCell<Vec<bool>>,
}

struct ArenaBorrow<'a, Ty, Key: ArenaKey> {
    inner: &'a mut Ty,

    #[cfg(debug_assertions)]
    parent: &'a ArenaCell<Ty, Key>,
    #[cfg(debug_assertions)]
    idx: Key,

    #[cfg(not(debug_assertions))]
    phantom: PhantomData<Key>,
}

impl<'a, Ty, Key: ArenaKey> AsRef<Ty> for ArenaBorrow<'a, Ty, Key> {
    fn as_ref(&self) -> &Ty {
        self.inner
    }
}

impl<'a, Ty, Key: ArenaKey> AsMut<Ty> for ArenaBorrow<'a, Ty, Key> {
    fn as_mut(&mut self) -> &mut Ty {
        self.inner
    }
}

impl<'a, Ty, Key: ArenaKey> Deref for ArenaBorrow<'a, Ty, Key> {
    type Target = Ty;

    fn deref(&self) -> &Self::Target {
        self.inner
    }
}

impl<'a, Ty, Key: ArenaKey> DerefMut for ArenaBorrow<'a, Ty, Key> {    
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner
    }
}

impl<'a, Ty, Key: ArenaKey> Drop for ArenaBorrow<'a, Ty, Key> {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        unsafe {
            let borrows = self.parent.borrowed.get().as_mut().unwrap();
            borrows[self.idx.to_usize().get() - 1] = false;
        }
    }
}

impl<Ty, Key: ArenaKey> ArenaCell<Ty, Key> {
    pub fn new() -> Self {
        ArenaCell {
            objects: UnsafeCell::new(Vec::new()),

            phantom: PhantomData{},

            #[cfg(debug_assertions)]
            borrowed: UnsafeCell::new(Vec::new()),
        }
    }

    pub fn add(&mut self, object: Ty) -> Key {
        self.objects.get_mut().push(object);
        
        #[cfg(debug_assertions)]
        self.borrowed.get_mut().push(false);

        unsafe { Key::from_index(self.objects.get_mut().len() - 1) }
    }

    pub fn get(&self, id: Key) -> &Ty {
        let idx = id.to_index();

        #[cfg(debug_assertions)]
        unsafe {
            let borrows = self.borrowed.get().as_mut().unwrap();
            if borrows[idx] {
                panic!("ArenaCell: double-borrow for {idx} (immutable borrow during mutable borrow)");
            }
        }

        unsafe { self.objects.get().as_mut().unwrap().get(idx).unwrap() }
    }

    pub fn get_mut(&self, id: Key) -> ArenaBorrow<Ty, Key> {
        let idx = id.to_index();

        #[cfg(debug_assertions)]
        unsafe {
            let borrows = self.borrowed.get().as_mut().unwrap();
            if borrows[idx] {
                panic!("ArenaCell: double-borrow for {idx} (second mutable borrow)");
            }
            borrows[idx] = true;
        }

        // This nonsense makes Miri happy
        let ptr = unsafe { self.objects.get().as_mut().unwrap().as_mut_ptr().add(idx).as_mut().unwrap() };

        // Perhaps the more natural expression:
        // let ptr = unsafe { self.exprs.get().as_mut().unwrap().get_mut(id).unwrap() };

        return ArenaBorrow {
            inner: ptr,
            #[cfg(debug_assertions)]
            parent: self,
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
            keys.push(arena.add(10));
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

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "ArenaCell: double-borrow for 0 (second mutable borrow)")]
    fn test_failure_mut() {
        let mut arena = ArenaCell::<i32, TestId>::new();
        let a = arena.add(5);
        arena.add(10);

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
        let a = arena.add(5);
        arena.add(10);

        let a_borrow = arena.get_mut(a);
        // should panic
        let a_borrow2 = arena.get(a);

        eprintln!("{} {}", *a_borrow, *a_borrow2);
    }
}