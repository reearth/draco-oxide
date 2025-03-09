use super::{
	Buffer, OrderConfig, RawBuffer
};

/// Writes the data to the buffer by borrowing it mutably.
/// Mostly used by the encoder.
pub struct Writer<Order: OrderConfig> {
	ptr: *mut u8,

	/// the number of bits written to the buffer.
	num_elements: usize,

	/// the number of elements written in the current byte.
	pos_in_curr_byte: usize,

	/// raw buffer to write to.
	/// we return it back to the buffer at the end.
	buffer: RawBuffer,

	_phantom: std::marker::PhantomData<Order>,
}

impl<Order: OrderConfig> Into<Buffer<Order>> for Writer<Order> {
	fn into(self) -> Buffer<Order> {
		Buffer::<Order> {
			data: self.buffer,
			len: self.num_elements,
			_phantom: std::marker::PhantomData,
		}
	}
}

impl<Order: OrderConfig> Writer<Order> {
	/// write the 'size' bits of data at the current offset.
	/// the input will be taken to be the first 'size' bits of 'value'.
	pub fn next(&mut self, (size, value): (usize, usize)) {
		// this not an unsafe condition, but it is a good practice to avoid weird inputs.
		assert!(size <= 64 && size > 0, "Invalid size: {}", size);

		// this is the unsafe condition. Allocate more memory if needed.
		while size + self.num_elements > self.buffer.cap << 3 {
			self.buffer.double();
		}

		// Safety: since 'self.num_elements' is less than 'self.buffer.cap << 3', we have
		// 'self.num_elements >> 3' <= '(self.buffer.cap << 3) >> 3' = 'self.buffer.cap',
		// so the  safety around 'self.buffer.cap' caries over this unsafe block.
		unsafe{ 
			self.ptr = self.buffer.as_ptr().add(self.num_elements >> 3);
		}

		unsafe{ self.next_unchecked((size, value)) }
	}

	/// write the 'size' bits of data at the current offset without checking the bounds.
	/// the first 'size' bits will be taken from 'value' as an input.
	/// Safety:  The caller must ensure that 'buffer' has allocated enough memory to store the data; 
	/// i.e. 'buffer.cap' is greater than or equal to 'num_elements'+'size'.
	pub unsafe fn next_unchecked(&mut self, (size, value): (usize, usize)) {
		self.num_elements = self.num_elements.unchecked_add(size);
		
		let mut offset = if Order::IS_MSB_FIRST{ size } else { 0 };
		
		if self.pos_in_curr_byte != 0 {
			let num_remaining_in_curr_byte = 8_usize.unchecked_sub(self.pos_in_curr_byte);
			if size <= num_remaining_in_curr_byte {
				unsafe {
					// Safety: dereferencing the pointer is safe because the condition implies that 
					// the data pointed to by it has been initialized.
					*self.ptr |= if Order::IS_MSB_FIRST {
						(value & ((1<<num_remaining_in_curr_byte)-1)) << num_remaining_in_curr_byte.unchecked_sub(size)
					} else {
						value << self.pos_in_curr_byte
					} as u8;
				}
				self.pos_in_curr_byte = if size == num_remaining_in_curr_byte {
					self.ptr = unsafe{ self.ptr.add(1) };
					0
				} else {
					self.pos_in_curr_byte.unchecked_add(size)
				};
				return;
			}
			unsafe {
				// Safety: dereferencing the pointer is safe because the condition implies that 
				// the data pointed to by it has been initialized.
				*self.ptr |= if Order::IS_MSB_FIRST {
					(value >> size.unchecked_sub(num_remaining_in_curr_byte)) & ((1<<num_remaining_in_curr_byte)-1)
				} else {
					value << self.pos_in_curr_byte
				} as u8;
			}
			self.ptr = self.ptr.add(1);
			offset = if Order::IS_MSB_FIRST {
				size.unchecked_sub(num_remaining_in_curr_byte)
			} else {
				num_remaining_in_curr_byte
			}
		}
		
		
		for _ in 0.. if Order::IS_MSB_FIRST{ offset } else { size.unchecked_sub(offset) } >> 3 {
			if Order::IS_MSB_FIRST{ offset = offset.unchecked_sub(8) };
			unsafe {
				self.ptr.write((value >> offset) as u8);
			}
			if !Order::IS_MSB_FIRST{ offset = offset.unchecked_add(8) };
			self.ptr = self.ptr.add(1);
		}
		unsafe {
			self.ptr.write(if Order::IS_MSB_FIRST {(value & ((1<<offset)-1))<<(8_usize.unchecked_sub(offset))} else {value >> offset} as u8);
		}
		self.pos_in_curr_byte = if Order::IS_MSB_FIRST {offset} else {size.unchecked_sub(offset) & 7};
	}

	pub fn new() -> Self {
		let buffer= RawBuffer::with_capacity(1);
		Self {
			ptr: buffer.as_ptr(),
			num_elements: 0,
			pos_in_curr_byte: 0,
			buffer,
			_phantom: std::marker::PhantomData,
		}
	}

	/// A constructor that allocates the specified size (in bits) beforehand.
	pub fn with_len(len: usize) -> Self {
		let cap = (len + 7) >> 3;
		let buffer = RawBuffer::with_capacity(cap);
		Self {
			ptr: buffer.data.as_ptr(),
			num_elements: 0,
			pos_in_curr_byte: 0,
			buffer,
			_phantom: std::marker::PhantomData,
		}
	}
}
