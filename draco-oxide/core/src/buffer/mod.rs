use crate::safety_assert;
pub mod attribute;

use std::{alloc, fmt, ptr};

pub trait OrderConfig {
    const IS_MSB_FIRST: bool;
}

#[derive(Debug)]
pub struct MsbFirst;

#[derive(Debug)]
pub struct LsbFirst;

impl OrderConfig for MsbFirst {
    const IS_MSB_FIRST: bool = true;
}
impl OrderConfig for LsbFirst {
    const IS_MSB_FIRST: bool = false;
}

struct RawBuffer {
    data: ptr::NonNull<u8>,

    /// the size of the allocation in bytes.
    /// The number of bits that can be stored in the buffer is 'cap' * 8.
    cap: usize,
}

impl RawBuffer {
    /// constructs a new buffer with the given capacity.
    /// 'cap' must be given in bytes.
    fn with_capacity(cap: usize) -> Self {
        let data = unsafe { alloc::alloc(alloc::Layout::array::<u8>(cap).unwrap()) };
        Self {
            data: ptr::NonNull::new(data).unwrap(),
            cap,
        }
    }

    /// expands the buffer to 'new_cap'.
    /// Safety: 'new_cap' must be less than 'usize::Max'.
    unsafe fn expand(&mut self, new_cap: usize) {
        safety_assert!(new_cap < usize::MAX, "'new_cap' is too large");
        let new_data = alloc::realloc(
            self.data.as_ptr(),
            alloc::Layout::array::<u8>(self.cap).unwrap(),
            new_cap,
        );
        self.data = ptr::NonNull::new(new_data).unwrap_or_else(|| {
            alloc::handle_alloc_error(alloc::Layout::array::<u8>(new_cap).unwrap())
        });
        self.cap = new_cap;
    }

    /// doubles the capacity of the buffer.
    fn double(&mut self) {
        let new_cap = self.cap * 2;
        assert!(new_cap < usize::MAX, "'new_cap' is too large");
        // Safety: Just checked that 'new_cap' is less than 'usize::Max'.
        unsafe {
            self.expand(new_cap);
        }
    }

    fn as_ptr(&self) -> *mut u8 {
        self.data.as_ptr()
    }

    fn from_vec<Data>(v: Vec<Data>) -> Self {
        let cap = v.len() * std::mem::size_of::<Data>();
        let data = v.as_ptr() as *mut u8;
        // forget the value to prevent double free
        std::mem::forget(v);
        Self {
            data: ptr::NonNull::new(data).unwrap(),
            cap,
        }
    }
}

impl fmt::Debug for RawBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for n in 0..self.cap {
            write!(f, "{:02x} ", unsafe { *self.data.as_ptr().add(n) })?;
        }
        Ok(())
    }
}
