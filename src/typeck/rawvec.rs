use std::{
    alloc::{alloc, realloc, Allocator, Global, Layout},
    cell::Cell,
    fmt,
    mem::{self, ManuallyDrop},
    ptr::{self, NonNull},
    slice::{self, Iter, IterMut},
    vec::IntoIter,
};

#[macro_export]
macro_rules! raw_vec {
    () => (
        $crate::typeck::rawvec::RawVec::with_cap(0)
    );
    ($($x:expr),*) => (
        $crate::typeck::rawvec::RawVec::from_vec(vec![$($x),*])
    );
    ($($x:expr,)*) => (raw_vec![$($x),*])
}

pub struct RawVec<T> {
    pub(crate) ptr: Cell<NonNull<T>>,
    pub(crate) cap: Cell<usize>,
    pub(crate) len: Cell<usize>,
}

impl<T> RawVec<T> {
    /// Make a new array with enough room to hold at least `cap` elements.
    #[inline]
    pub fn with_cap(cap: usize) -> Self {
        unsafe {
            if mem::size_of::<T>() == 0 || cap == 0 {
                RawVec {
                    ptr: Cell::new(NonNull::new_unchecked(ptr::null_mut())),
                    len: Cell::new(0),
                    cap: Cell::new(cap),
                }
            } else {
                let ptr = {
                    let layout = Layout::array::<T>(cap).unwrap();

                    alloc(layout) as *mut T
                };

                RawVec {
                    ptr: Cell::new(NonNull::new_unchecked(ptr)),
                    len: Cell::new(0),
                    cap: Cell::new(cap),
                }
            }
        }
    }

    #[inline]
    pub fn from_vec(v: Vec<T>) -> RawVec<T> {
        // We are now responsible for the memory, we own the buffer
        let (p, l, c) = v.into_raw_parts();
        unsafe { RawVec::from_raw_parts(p, l, c) }
    }

    #[inline]
    pub fn into_iter(self) -> IntoIter<T> {
        // We need to pass ownership to the `Vec`, it is now responsible for destructor-ing
        // This happens because `Vec::from_raw_parts` does NOT take ownership
        let x = ManuallyDrop::new(self);
        unsafe { Vec::from_raw_parts(x.ptr(), x.len(), x.cap()).into_iter() }
    }

    /// Return number of elements array contains.
    #[inline]
    pub fn len(&self) -> usize {
        self.len.get()
    }

    /// Return number of elements array contains.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Return number of elements array can hold before reallocation.
    #[inline]
    pub fn cap(&self) -> usize {
        self.cap.get()
    }

    #[inline]
    pub fn slice_mut(&mut self) -> &mut [T] {
        unsafe { slice::from_raw_parts_mut(self.ptr(), self.len()) }
    }

    #[inline]
    pub fn slice(&self) -> &[T] {
        unsafe { slice::from_raw_parts(self.ptr(), self.len()) }
    }

    #[inline]
    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        self.slice_mut().iter_mut()
    }

    #[inline]
    pub fn iter(&self) -> Iter<'_, T> {
        self.slice().iter()
    }

    #[inline]
    pub fn ptr(&self) -> *mut T {
        self.ptr.get().as_ptr()
    }

    pub fn grow(&self, cap: usize) {
        if self.cap() == 0 && cap > 0 {
            let new_ptr = unsafe {
                let layout = Layout::array::<T>(cap).unwrap();

                alloc(layout) as *mut T
            };

            self.cap.set(cap);
            self.ptr.set(NonNull::new(new_ptr).unwrap());
            return;
        }

        if mem::size_of::<T>() > 0 && cap > self.cap() {
            unsafe {
                let layout = Layout::array::<T>(self.cap()).unwrap();

                let new_ptr = realloc(self.ptr() as *mut u8, layout, cap) as *mut T;
                self.cap.set(cap);
                self.ptr.set(NonNull::new(new_ptr).unwrap());
            }
        }
    }

    #[inline]
    pub fn push(&mut self, item: T) {
        unsafe { self.push_shared(item) }
    }

    #[inline]
    pub unsafe fn push_shared(&self, item: T) {
        if self.len() == self.cap() {
            self.grow(4);
        }

        unsafe {
            let end = self.ptr().add(self.len());
            ptr::write(end, item);
            self.len.set(self.len() + 1);
        }
    }

    #[inline]
    pub fn pop(&mut self) -> Option<T> {
        if self.is_empty() {
            None
        } else {
            unsafe {
                self.len.set(self.len() - 1);
                Some(ptr::read(self.ptr().add(self.len())))
            }
        }
    }

    #[inline]
    pub fn remove(&mut self, index: usize) -> T {
        #[cold]
        #[inline(never)]
        #[track_caller]
        fn assert_failed(index: usize, len: usize) -> ! {
            panic!("removal index (is {}) should be < len (is {})", index, len);
        }

        let len = self.len();
        if index >= len {
            assert_failed(index, len);
        }
        unsafe {
            // infallible
            let ret;
            {
                // the place we are taking from.
                let ptr = self.ptr().add(index);
                // copy it out, unsafely having a copy of the value on
                // the stack and in the vector at the same time.
                ret = ptr::read(ptr);

                // Shift everything down to fill in that spot.
                ptr::copy(ptr.offset(1), ptr, len - index - 1);
            }
            self.len.set(len - 1);
            ret
        }
    }

    #[inline]
    pub unsafe fn from_raw_parts(ptr: *mut T, len: usize, cap: usize) -> Self {
        RawVec {
            ptr: Cell::new(NonNull::new_unchecked(ptr)),
            len: Cell::new(len),
            cap: Cell::new(cap),
        }
    }
}

#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl<T: Send> Send for RawVec<T> {}
unsafe impl<T: Sync> Sync for RawVec<T> {}

impl<T: Clone> Clone for RawVec<T> {
    fn clone(&self) -> Self {
        // Actually clone this not just give another pointer oops
        //
        // I was `RawVec { ptr: self.ptr()... }` WRONG
        let slice = self.slice();
        RawVec::from_vec(slice.to_vec())
    }
}

impl<T: fmt::Debug> fmt::Debug for RawVec<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.slice().fmt(f)
    }
}

impl<T: std::hash::Hash> std::hash::Hash for RawVec<T> {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(self.slice(), state)
    }
}

impl<T: PartialEq> PartialEq for RawVec<T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        PartialEq::eq(self.slice(), other.slice())
    }
}

impl<T: Eq> Eq for RawVec<T> {}

impl<T> Drop for RawVec<T> {
    fn drop(&mut self) {
        // Safety:
        // We are the only owner so go nuts
        unsafe {
            let ptr = self.ptr();
            // Give ownership of `Self` to `Vec` to clean up
            Vec::from_raw_parts(ptr, self.len(), self.cap());
        }
    }
}

#[test]
fn raw_vec() {
    let mut list = RawVec::with_cap(5);

    list.push(10);
    list.push(9);
    list.push(8);
    list.push(7);

    assert_eq!(list.pop(), Some(7));
    assert_eq!(list.pop(), Some(8));
    assert_eq!(list.pop(), Some(9));

    for i in list.slice().iter() {
        assert_eq!(10, *i);
    }
}

#[test]
fn raw_vec_macro() {
    let mut list = crate::raw_vec![1, 2, 3, 4, 5, 6, 7];

    assert_eq!(list.pop(), Some(7));
}

#[test]
fn raw_vec_iter_mut() {
    let mut list = crate::raw_vec![1, 2, 3, 4, 5, 6, 7];
    let mut cmp_list = crate::raw_vec![1, 2, 3, 4, 5, 6, 7];

    for x in list.iter_mut() {
        *x += 1;
    }
    for i in list.iter() {
        assert_eq!(cmp_list.remove(0) + 1, *i)
    }
}

#[test]
fn raw_vec_zero_init() {
    let mut list = RawVec::with_cap(0);

    list.push(10);
    list.push(9);
    list.push(8);
    list.push(7);

    assert_eq!(list.pop(), Some(7));
    assert_eq!(list.pop(), Some(8));
    assert_eq!(list.pop(), Some(9));

    for i in list.slice().iter() {
        assert_eq!(10, *i);
    }
}

#[test]
fn test_from_iter_specialization_panic_during_drop_leaks() {
    static mut DROP_COUNTER: usize = 0;

    #[derive(Debug)]
    enum Droppable {
        DroppedTwice(Box<i32>),
        PanicOnDrop,
    }

    impl Drop for Droppable {
        fn drop(&mut self) {
            match self {
                Droppable::DroppedTwice(_) => unsafe {
                    DROP_COUNTER += 1;
                },
                Droppable::PanicOnDrop => {
                    if !std::thread::panicking() {
                        panic!();
                    }
                }
            }
        }
    }

    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let v = crate::raw_vec![Droppable::DroppedTwice(Box::new(123)), Droppable::PanicOnDrop];
        let _ = v.into_iter().take(0).collect::<Vec<_>>();
    }));

    assert_eq!(unsafe { DROP_COUNTER }, 1);
}

#[test]
fn raw_vec_from() {
    let list = vec![1, 2, 3, 4, 5, 6, 7];
    let mut raw = RawVec::from_vec(list);
    let _iter = raw.into_iter().take(1).collect::<Vec<_>>();
}

#[test]
fn raw_vec_from_empty() {
    let list: Vec<u8> = vec![];
    let mut raw = RawVec::from_vec(list);
}
