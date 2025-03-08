use super::RawBuffer;
use std::ptr;

/// Reads the data of the buffer by consuming the buffer.
/// Mostly used by the decoder.
pub struct Reader {
	ptr: *const u8,

	/// the number of bits remaining in the buffer.
	num_remaining_elements: usize,

	/// the position in the current byte being read. It is always less than 8.
	pos_in_curr_byte: usize,

	/// The buffer. We want him to die at the very end for deallocation.
	_buffer: RawBuffer,
}


impl Reader {
	/// read the 'size' bits of data at the current offset.
	/// the output data is stored in the first 'size' bits.
	pub fn next(&mut self, size: usize) -> usize {
		assert!(size <= 64 && size > 0, "Invalid size: {}", size);
		assert!(size <= self.num_remaining_elements, "Attempt to read beyond buffer bounds");
			unsafe{ self.next_unchecked(size) }
	}

	/// read the 'size' bits of data at the current offset without checking the bounds.
	/// the output data is stored in the first 'size' bits.
	/// Safety:  The caller must ensure that 
	///  (1) 'size' is less than or equal to 'self.num_remaining_elements'.
	///  (2) 'size' is less than or equal to 64.
	pub unsafe fn next_unchecked(&mut self, size: usize) -> usize {
		self.num_remaining_elements = self.num_remaining_elements.unchecked_sub(size);
		
		let mut offset = 0;
		let mut value = 0;
		
		if self.pos_in_curr_byte != 0 {
			let num_remaining_in_curr_byte = 8 - self.pos_in_curr_byte;
			value = (unsafe{ ptr::read(self.ptr) } >> self.pos_in_curr_byte) as usize;
			if size < num_remaining_in_curr_byte { 
				self.pos_in_curr_byte = self.pos_in_curr_byte.unchecked_add(size);
				return value&((1<<size)-1);
			} 
			
			self.ptr = unsafe{ self.ptr.add(1) };
			if size == num_remaining_in_curr_byte {
				self.pos_in_curr_byte = 0;
				return value;
			}
			offset = num_remaining_in_curr_byte;
		}
		
		
		for _ in 0..size.unchecked_sub(offset) >> 3 {
			value |= (unsafe{ ptr::read(self.ptr) } << offset) as usize;
			offset = offset.unchecked_add(8);
			self.ptr = unsafe{ self.ptr.add(1) };
		}

		// 'size'-'offset' is the number of bits remaining to be read.
		value |= (unsafe{ ptr::read(self.ptr) as usize } & ((1<<size.unchecked_sub(offset))-1)) << offset;
		self.pos_in_curr_byte = (size-offset) & 7;

		
		value
	}

	pub(super) fn new(buffer: RawBuffer) -> Self {
		let ptr = buffer.data.as_ptr();
		Self {
			ptr,
			num_remaining_elements: buffer.cap << 3,
			pos_in_curr_byte: 0,
			_buffer: buffer,
		}
	}
}