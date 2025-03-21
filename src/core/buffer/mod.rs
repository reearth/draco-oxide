pub mod attriute;
pub mod encoder;
pub mod decoder;
pub mod reader;
pub mod writer;

use std::{
    alloc, fmt, ptr
};

use reader::Reader;

trait OrderConfig{
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

#[derive(Debug)]
pub struct Buffer<Order: OrderConfig = MsbFirst> {
	data: RawBuffer,

	/// length of the buffer, i.e. the number of bits stored in the buffer.
    /// The minimum number of bytes allocated for the buffer is 'len' / 8.
	len: usize,

    _phantom: std::marker::PhantomData<Order>,
}

impl<Order: OrderConfig> Buffer <Order> {
	/// constructs an empty buffer
    pub fn new() -> Self {
        Self { data: RawBuffer::new(), len: 0, _phantom: std::marker::PhantomData }
    }
	
	/// A constructor that allocates the specified size (in bits) beforehand.
	pub fn with_len(len: usize) -> Self {
        let cap = (len + 7) >> 3;
        let data = RawBuffer::with_capacity(cap);
        Self { data, len, _phantom: std::marker::PhantomData }
    }


    /// returns the number of bits stored in the buffer.
    pub fn len(&self) -> usize {
        self.len
    }

    /// returns the reader for the buffer.
	pub fn into_reader(self) -> Reader<Order> {
        Reader::new(self.data)
    }
}

struct RawBuffer {
    data: ptr::NonNull<u8>,

    /// the size of the allocation.
    /// The number of bits that can be stored in the buffer is 'cap' * 8.
    cap: usize,
}

impl RawBuffer {
    fn new() -> Self {
        Self { data: ptr::NonNull::dangling(), cap: 0 }
    }

    fn with_capacity(cap: usize) -> Self {
        let data = unsafe { alloc::alloc(alloc::Layout::array::<u8>(cap).unwrap()) };
        Self { data: ptr::NonNull::new(data).unwrap(), cap }
    }

    /// expands the buffer to 'new_cap'.
    /// Safety: 'new_cap' must be less than 'usize::Max'.
    unsafe fn expand(&mut self, new_cap: usize) {
        debug_assert!(new_cap < usize::MAX, "'new_cap' is too large");
        let new_data = 
            alloc::realloc(self.data.as_ptr() as *mut u8, alloc::Layout::array::<u8>(self.cap).unwrap(), new_cap);
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
        unsafe{ self.expand(new_cap); }
    }

    fn as_ptr(&self) -> *mut u8 {
        self.data.as_ptr()
    }
}

impl fmt::Debug for RawBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for n in 0..self.cap {
            write!(f, "{:02x} ", unsafe{ *self.data.as_ptr().add(n) })?;
        }
        Ok(())
    }
}


#[cfg(test)]
mod tests {
    use crate::core::buffer::*;

    #[test]
    fn test_writer_reader_msb_first() {
        let mut writer = writer::Writer::<MsbFirst>::new();
        writer.next((7, 0b0111010));
        let buffer: Buffer<_> = writer.into();
        assert_eq!(buffer.len(), 7);
        let mut reader: Reader<_> = buffer.into_reader();
        assert_eq!(reader.next(7), 0b0111010);

        let mut writer = writer::Writer::<MsbFirst>::new();
        writer.next((8, 0b10111010));
        let buffer: Buffer<_> = writer.into();
        assert_eq!(buffer.len(), 8);
        let mut reader: Reader<_> = buffer.into_reader();
        assert_eq!(reader.next(8), 0b10111010);

        let mut writer = writer::Writer::<MsbFirst>::new();
        writer.next((9, 0b110111010));
        let buffer: Buffer<_> = writer.into();
        assert_eq!(buffer.len(), 9);
        let mut reader: Reader<_> = buffer.into_reader();
        assert_eq!(reader.next(9), 0b110111010);
        
        let mut writer = writer::Writer::<MsbFirst>::new();
        writer.next((9, 0b101010100));
        writer.next((8, 0b10101010));
        writer.next((7, 0b0101010));
        writer.next((6, 0b111100));
        writer.next((5, 0b00001));
        writer.next((4, 0b1100));
        let buffer: Buffer<_> = writer.into();
        assert_eq!(buffer.len(), 9+8+7+6+5+4);
        let mut reader: Reader<_> = buffer.into_reader();
        assert_eq!(reader.next(9), 0b101010100);
        assert_eq!(reader.next(8), 0b10101010);
        assert_eq!(reader.next(7), 0b0101010);
        assert_eq!(reader.next(6), 0b111100);
        assert_eq!(reader.next(5), 0b00001);
        assert_eq!(reader.next(4), 0b1100);
        
        let mut writer = writer::Writer::<MsbFirst>::new();
        writer.next((11, 0b10111010110));
        let buffer: Buffer<_> = writer.into();
        assert_eq!(buffer.len(), 11);
        let mut reader = buffer.into_reader();
        assert_eq!(reader.next(2), 0b10);
        assert_eq!(reader.next(1), 0b1);
        assert_eq!(reader.next(3), 0b110);
        assert_eq!(reader.next(3), 0b101);
        assert_eq!(reader.next(2), 0b10);

    }

    #[test]
    fn test_writer_reader_lsb_first() {
        let mut writer = writer::Writer::<LsbFirst>::new();
        writer.next((9, 0b101010100));
        writer.next((8, 0b10101010));
        writer.next((7, 0b0101010));
        writer.next((6, 0b111100));
        writer.next((5, 0b00001));
        writer.next((4, 0b1100));
        let buffer: Buffer<_> = writer.into();
        assert_eq!(buffer.len(), 9+8+7+6+5+4);
        let mut reader = buffer.into_reader();
        assert_eq!(reader.next(9), 0b101010100);
        assert_eq!(reader.next(8), 0b10101010);
        assert_eq!(reader.next(7), 0b0101010);
        assert_eq!(reader.next(6), 0b111100);
        assert_eq!(reader.next(5), 0b00001);
        assert_eq!(reader.next(4), 0b1100);

        let mut writer = writer::Writer::<LsbFirst>::new();
        writer.next((10, 0b1010101010));
        let buffer: Buffer<_> = writer.into();
        assert_eq!(buffer.len(), 10);
        let mut reader = buffer.into_reader();
        for _ in 0..5 {
            assert_eq!(reader.next(2), 0b10);
        }
    }

    #[test]
    fn test_writer_reader_unchecked() {
        let mut writer = writer::Writer::<LsbFirst>::with_len(9+8+7+6+5+4);
        unsafe{
            writer.next_unchecked((9, 0b10101010<<1));
            writer.next_unchecked((8, 0b10101010));
            writer.next_unchecked((7, 0b0101010));
            writer.next_unchecked((6, 0b111100));
            writer.next_unchecked((5, 0b00001));
            writer.next_unchecked((4, 0b1100));
            let buffer: Buffer<_> = writer.into();
            assert_eq!(buffer.len(), 9+8+7+6+5+4);
            let mut reader = buffer.into_reader();
            assert_eq!(reader.next_unchecked(9), 0b10101010<<1);
            assert_eq!(reader.next_unchecked(8), 0b10101010);
            assert_eq!(reader.next_unchecked(7), 0b0101010);
            assert_eq!(reader.next_unchecked(6), 0b111100);
            assert_eq!(reader.next_unchecked(5), 0b00001);
            assert_eq!(reader.next_unchecked(4), 0b1100);
        }
    }
}