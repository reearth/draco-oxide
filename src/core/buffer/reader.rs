use super::{OrderConfig, RawBuffer, MsbFirst};
use std::ptr;

/// Reads the data of the buffer by consuming the buffer.
/// Mostly used by the decoder.
pub struct Reader<Order: OrderConfig = MsbFirst> {
	ptr: *const u8,

	/// the number of bits remaining in the buffer.
	num_remaining_bits: usize,

	/// the position in the current byte being read. It is always less than 8.
	pos_in_curr_byte: u8,

	/// The buffer. We want him to die at the very end for deallocation.
	_buffer: RawBuffer,

	_phantom: std::marker::PhantomData<Order>,
}


impl<Order: OrderConfig> Reader<Order> {
	/// read the 'size' bits of data at the current offset.
	/// the output data is stored in the first 'size' bits.
	pub fn next(&mut self, size: u8) -> u64 {
		assert!(size <= 64 && size > 0, "Invalid size: {}", size);
		assert!(size as usize <= self.num_remaining_bits, "Attempt to read beyond buffer bounds");
			unsafe{ self.next_unchecked(size) }
	}

	/// read the 'size' bits of data at the current offset without checking the bounds.
	/// the output data is stored in the first 'size' bits.
	/// Safety:  The caller must ensure that 

	///  (1) 'size' is less than or equal to 'self.num_remaining_bits'.
	///  (2) 'size' is less than or equal to 64.
	pub unsafe fn next_unchecked(&mut self, size: u8) -> u64 {
		self.num_remaining_bits = self.num_remaining_bits.unchecked_sub(size as usize);
		
		let mut offset = if Order::IS_MSB_FIRST{ size } else { 0 };
		let mut value: u64 = 0;
		
		if self.pos_in_curr_byte != 0 {
			let num_remaining_in_curr_byte = 8 - self.pos_in_curr_byte;
			if size <= num_remaining_in_curr_byte { 
				value = unsafe {
					if Order::IS_MSB_FIRST {
						(ptr::read(self.ptr) & ((1<<num_remaining_in_curr_byte)-1)) >> (num_remaining_in_curr_byte.unchecked_sub(size))
					} else {
						ptr::read(self.ptr) >> self.pos_in_curr_byte
					}
				} as u64;
				self.pos_in_curr_byte = if size == num_remaining_in_curr_byte {
					self.ptr = unsafe{ self.ptr.add(1) };
					0
				} else {
					self.pos_in_curr_byte.unchecked_add(size)
				};
				return value&((1<<size)-1);
			}
			value = unsafe {
				if Order::IS_MSB_FIRST {
					((ptr::read(self.ptr) as usize) & ((1<<num_remaining_in_curr_byte)-1)) << size.unchecked_sub(num_remaining_in_curr_byte)
				} else {
					(ptr::read(self.ptr) >> self.pos_in_curr_byte) as usize
				}
			} as u64;
			self.ptr = unsafe{ self.ptr.add(1) };
			offset = if Order::IS_MSB_FIRST {
				offset.unchecked_sub(num_remaining_in_curr_byte)
			} else {
				num_remaining_in_curr_byte
			};
		}
		
		
		for _ in 0..if Order::IS_MSB_FIRST{ offset } else { size.unchecked_sub(offset) } >> 3 {
			if Order::IS_MSB_FIRST {
				offset = offset.unchecked_sub(8);
			}
			value |= unsafe{ ptr::read(self.ptr) as u64 } << offset;
			if !Order::IS_MSB_FIRST {
				offset = offset.unchecked_add(8);
			}
			self.ptr = unsafe{ self.ptr.add(1) };
		}

		// 'size'-'offset' is the number of bits remaining to be read.
		value |= unsafe{ 
			if Order::IS_MSB_FIRST {
				(ptr::read(self.ptr) as u64 >> (8_u8.unchecked_sub(offset))) & ((1<<offset)-1)
			} else {
				(ptr::read(self.ptr) as u64 & ((1<<size.unchecked_sub(offset))-1)) << offset
			}
		};

		self.pos_in_curr_byte = if Order::IS_MSB_FIRST {
			offset
		} else {
			(size-offset) & 7
		};
		value
	}

	pub(super) fn new(buffer: RawBuffer) -> Self {
		let ptr = buffer.data.as_ptr();
		Self {
			ptr,
			num_remaining_bits: buffer.cap << 3,
			pos_in_curr_byte: 0,
			_buffer: buffer,
			_phantom: std::marker::PhantomData,
		}
	}
}