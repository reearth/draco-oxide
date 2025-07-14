use crate::core::shared::{BitWriter, ByteWriter};

use super::{
	Buffer, MsbFirst, OrderConfig, RawBuffer
};

/// Writes the data to the buffer by borrowing it mutably.
/// Mostly used by the encoder.
#[derive(Debug)]
#[repr(C)]
pub struct Writer<Order: OrderConfig = MsbFirst, const BIT_CODER_ENABLED: bool = false> {
	ptr: *mut u8,

	/// the number of bits written to the buffer.
	num_bits: usize,

	/// the number of elements written in the current byte.
	pos_in_curr_byte: u8,

	/// raw buffer to write to.
	/// we return it back to the buffer at the end.
	buffer: RawBuffer,

	_phantom: std::marker::PhantomData<Order>,
}

impl<Order: OrderConfig, const BIT_CODER_ENABLED: bool> Into<Buffer<Order>> for Writer<Order, BIT_CODER_ENABLED> {
	fn into(self) -> Buffer<Order> {
		Buffer::<Order> {
			data: self.buffer,
			len: self.num_bits,
			_phantom: std::marker::PhantomData,
		}
	}
}

impl<Order: OrderConfig> BitWriter for Writer<Order, true> {
	type ByteWriter = Writer<Order, false>;
	/// write the 'size' bits of data at the current offset.
	/// the input will be taken to be the first 'size' bits of 'value'.
	fn write_bits(&mut self, (size, value): (u8, u64)) {
		// this not an unsafe condition, but it is a good practice to avoid weird inputs.
		assert!(size <= 64 && size > 0, "Invalid size: {}", size);

		// this is the unsafe condition. Allocate more memory if needed.
		while size as usize + self.num_bits >= self.buffer.cap << 3 {
			self.buffer.double();
		}

		// Safety: since 'self.num_bits' is less than 'self.buffer.cap << 3', we have
		// 'self.num_bits >> 3' <= '(self.buffer.cap << 3) >> 3' = 'self.buffer.cap',
		// so the  safety around 'self.buffer.cap' caries over this unsafe block.
		unsafe{ 
			self.ptr = self.buffer.as_ptr().add(self.num_bits >> 3);
		}

		// First 'size' bits of 'value' need to contain the data.
		debug_assert!(
			size==64 || value >> size==0,
			"Invalid Data: 'value' has more than 'size' bits of data: {:?}",
			(size, value)
		);

		unsafe{ self.write_bits_unchecked((size, value)) }
	}

	fn into_byte_writer(&mut self) -> &mut Self::ByteWriter {
		// clean up the current byte and reaturn byte writer.
		if self.pos_in_curr_byte != 0 {
			if self.num_bits + 8 >= self.buffer.cap << 3 {
				self.buffer.double();
			}
			self.ptr = unsafe{ self.ptr.add(1) };
			self.num_bits = self.num_bits + (8 - self.pos_in_curr_byte as usize);
			self.pos_in_curr_byte = 0;
		}
		// Safety: we forced the memory layout of `Writer<Order, true>` and `Writer<Order, false>` to be the same.
		unsafe {
			&mut *(self as *mut Self as *mut Writer<Order, false>)
		}
	}
}

impl<Order: OrderConfig> ByteWriter for Writer<Order, false> {
	type BitWriter = Writer<Order, true>;

	fn write_byte(&mut self, data: u8) {
		// Safety: we have ensured that the raw buffer has enough capacity to write a byte.
		unsafe{
			self.ptr.write(data);
		}

		while self.num_bits + 8 >= self.buffer.cap << 3 {
			self.buffer.double();
		}

		// Safety: We have ensured that the raw buffer has enough capacity to write a byte.
		unsafe {
			self.ptr = self.ptr.add(1);
		}

		self.num_bits += 8;
	}

	fn into_bit_writer(&mut self) -> &mut Self::BitWriter {
		let ptr = self as *mut Self;
		// Safety: we forced the memory layout of `Writer<Order, true>` and `Writer<Order, false>` to be the same.
		unsafe {
			&mut *(ptr as *mut Writer<Order, true>)
		}
	}
}

impl<Order: OrderConfig> Writer<Order, true> {
	/// write the 'size' bits of data at the current offset without checking the bounds.
	/// the first 'size' bits will be taken from 'value' as an input.
	/// Safety:  The caller must ensure that 'buffer' has allocated enough memory to store the data; 
	/// i.e. 'buffer.cap' is greater than or equal to 'num_bits'+'size'.
	pub unsafe fn write_bits_unchecked(&mut self, (size, value): (u8, u64)) {
		self.num_bits = self.num_bits.unchecked_add(size as usize);
		
		let mut offset = if Order::IS_MSB_FIRST{ size } else { 0 };
		
		if self.pos_in_curr_byte != 0 {
			let num_remaining_in_curr_byte = 8_u8.unchecked_sub(self.pos_in_curr_byte);
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
			self.ptr.write(if Order::IS_MSB_FIRST {(value & ((1<<offset)-1))<<(8_u8.unchecked_sub(offset))} else {value >> offset} as u8);
		}
		self.pos_in_curr_byte = if Order::IS_MSB_FIRST {offset} else {size.unchecked_sub(offset) & 7};
	}
}

impl<Order: OrderConfig, const BIT_CODER_ENABLED: bool> Writer<Order, BIT_CODER_ENABLED> {
	pub fn new() -> Self {
		let buffer= RawBuffer::with_capacity(1);
		Self {
			ptr: buffer.as_ptr(),
			num_bits: 0,
			pos_in_curr_byte: 0,
			buffer,
			_phantom: std::marker::PhantomData,
		}
	}

	/// A constructor that allocates the specified size (in bits) beforehand.
	pub fn with_cap(len: usize) -> Self {
		let cap = (len + 7) >> 3;
		let buffer = RawBuffer::with_capacity(cap);
		Self {
			ptr: buffer.data.as_ptr(),
			num_bits: 0,
			pos_in_curr_byte: 0,
			buffer,
			_phantom: std::marker::PhantomData,
		}
	}
}

