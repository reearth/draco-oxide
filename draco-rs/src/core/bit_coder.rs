use std::{iter::Rev, vec};

use super::buffer::{MsbFirst, OrderConfig};

pub trait ByteWriter: Sized {
    fn write_u8(&mut self, value: u8);
    fn write_u16(&mut self, value: u16) {
        self.write_u8(value as u8);
        self.write_u8((value >> 8) as u8);
    }
    fn write_u24(&mut self, value: u32) {
        self.write_u8(value as u8);
        self.write_u8((value >> 8) as u8);
        self.write_u8((value >> 16) as u8);
    }
    fn write_u32(&mut self, value: u32) {
        self.write_u16(value as u16);
        self.write_u16((value >> 16) as u16);
    }
    fn write_u64(&mut self, value: u64) {
        self.write_u32(value as u32);
        self.write_u32((value >> 32) as u32);
    }
}

impl ByteWriter for Vec<u8> {
    fn write_u8(&mut self, value: u8) {
        self.push(value);
    }

    fn write_u16(&mut self, value: u16) {
        self.extend_from_slice(&value.to_le_bytes());
    }

    fn write_u24(&mut self, value: u32) {
        self.extend_from_slice(&value.to_le_bytes()[..3]);
    }

    fn write_u32(&mut self, value: u32) {
        self.extend_from_slice(&value.to_le_bytes());
    }

    fn write_u64(&mut self, value: u64) {
        self.extend_from_slice(&value.to_le_bytes());
    }
}

pub struct FunctionalByteWriter<R> {
    write_fn: R,
}

impl<R: FnMut(u8)> ByteWriter for FunctionalByteWriter<R> {
    fn write_u8(&mut self, value: u8) {
        (self.write_fn)(value);
    }

    fn write_u16(&mut self, value: u16) {
        // ToDo: use unsafe code to avoid the allocation of the array.
        self.write_u8(value as u8);
        self.write_u8((value >> 8) as u8);
    }

    fn write_u24(&mut self, value: u32) {
        self.write_u8(value as u8);
        self.write_u8((value >> 8) as u8);
        self.write_u8((value >> 16) as u8);
    }

    fn write_u32(&mut self, value: u32) {
        self.write_u16(value as u16);
        self.write_u16((value >> 16) as u16);
    }

    fn write_u64(&mut self, value: u64) {
        self.write_u32(value as u32);
        self.write_u32((value >> 32) as u32);
    }
}

impl <R: FnMut(u8)> FunctionalByteWriter<R> {
    pub fn new(write_fn: R) -> Self {
        FunctionalByteWriter { write_fn }
    }
}

pub struct BitWriter<'buffer, Buffer: ByteWriter, Order: OrderConfig = MsbFirst> {
    buffer: &'buffer mut Buffer,

    /// The position in the current byte where the next bit will be written.
    /// It is always less than 8.
    pos_in_curr_byte: u8,

    /// The current byte being written to. This byte will be written to the buffer when it is full.
    curr_byte: u8,
    
    phantom: std::marker::PhantomData<Order>,
}

impl<'buffer, Buffer: ByteWriter, Order: OrderConfig> BitWriter<'buffer, Buffer, Order> {
    pub fn spown_from(buffer: &mut Buffer) -> BitWriter<'_, Buffer, Order> {
        BitWriter {
            buffer,
            pos_in_curr_byte: 0,
            curr_byte: 0,
            phantom: std::marker::PhantomData,
        }
    }

    pub fn write_bits(&mut self, (size, value): (u8, u64)) {
		let mut offset = if Order::IS_MSB_FIRST{ size } else { 0 };
		
		if self.pos_in_curr_byte != 0 {
            // Safety: by definition, `pos_in_curr_byte` is always less than 8.
			let num_remaining_in_curr_byte = unsafe{ 8_u8.unchecked_sub(self.pos_in_curr_byte) };
			if size <= num_remaining_in_curr_byte {
                self.curr_byte |= if Order::IS_MSB_FIRST {
                    // Safety: `checked by the if condition above.
                    unsafe {
                        (value & ((1<<num_remaining_in_curr_byte)-1)) << num_remaining_in_curr_byte.unchecked_sub(size)
                    }
                } else {
                    value << self.pos_in_curr_byte
                } as u8;
				self.pos_in_curr_byte = if size == num_remaining_in_curr_byte {
					self.buffer.write_u8( self.curr_byte );
                    self.curr_byte = 0;
					0
				} else {
                    unsafe { self.pos_in_curr_byte.unchecked_add(size) }
				};
				return;
			}
            self.curr_byte |= if Order::IS_MSB_FIRST {
                // Safety: 'num_remaining_in_curr_byte' is guaranteed to be less than 8.
                unsafe {
                    (value >> size.unchecked_sub(num_remaining_in_curr_byte)) & ((1<<num_remaining_in_curr_byte)-1)
                }
            } else {
                value << self.pos_in_curr_byte
            } as u8;
			self.buffer.write_u8(self.curr_byte);
            self.curr_byte = 0;
			offset = if Order::IS_MSB_FIRST {
                // Safety: `checked by the if condition above.
                unsafe { size.unchecked_sub(num_remaining_in_curr_byte) }
			} else {
				num_remaining_in_curr_byte
			}
		}
		
        // ToDo: Change the following and avoid the loop
        // Safety: 
        // In the case of MSB first. 'offset' is the number of bits remaining to write, and we iterate only 'offset/8' times 
        // means its safe to subtract 8 for each iteration.
        // In the case of LSB first, 'offset' is the number of bits written so far, so even if we add 8 to it '(size-offset)/8' times,
        // it is at most 'size', which is of type 'u8'. Hence it will never overflow.
		for _ in 0.. if Order::IS_MSB_FIRST{ offset } else { unsafe{ size.unchecked_sub(offset) } } >> 3 { unsafe {
			if Order::IS_MSB_FIRST{ offset = offset.unchecked_sub(8) };
            self.buffer.write_u8((value >> offset) as u8);
			if !Order::IS_MSB_FIRST{ offset = offset.unchecked_add(8) };
		}}
		self.curr_byte = if Order::IS_MSB_FIRST {
            // Safety: 'offset' is guaranteed to be less than or equal to 8 due to the previous loop.
            unsafe{ (value & ((1<<offset)-1))<<(8_u8.unchecked_sub(offset)) }
        } else {
            value >> offset
        } as u8;
		self.pos_in_curr_byte = if Order::IS_MSB_FIRST {
            offset
        } else {
            // Safety: 'size-offset' is guaranteed to be positive.
            unsafe{ size.unchecked_sub(offset) & 7 }
        };
    }
}

impl<'buffer, Buffer: ByteWriter, Order: OrderConfig> Drop for BitWriter<'buffer, Buffer, Order> {
    fn drop(&mut self) {
        // If there are bits left in the current byte, pad the current byte by writing the current data.
        if self.pos_in_curr_byte > 0 {
            self.buffer.write_u8(self.curr_byte);
        }
    }
}

pub trait ByteReader {
    type Rev: ReverseByteReader;
    fn read_u8(&mut self) -> Result<u8, ReaderErr>;
    fn read_u16(&mut self) -> Result<u16, ReaderErr> {
        let out = [
            self.read_u8()?,
            self.read_u8()?
        ];
        Ok(u16::from_le_bytes(out))
    }
    fn read_u24(&mut self) -> Result<u32, ReaderErr> {
        let out = [
            self.read_u8()?,
            self.read_u8()?,
            self.read_u8()?
        ];
        Ok(u32::from_le_bytes([out[0], out[1], out[2], 0]))
    }
    fn read_u32(&mut self) -> Result<u32, ReaderErr> {
        let out = [
            self.read_u8()?,
            self.read_u8()?,
            self.read_u8()?,
            self.read_u8()?
        ];
        Ok(u32::from_le_bytes(out))
    }
    fn read_u64(&mut self) -> Result<u64, ReaderErr> {
        let out = [
            self.read_u8()?,
            self.read_u8()?,
            self.read_u8()?,
            self.read_u8()?,
            self.read_u8()?,
            self.read_u8()?,
            self.read_u8()?,
            self.read_u8()?
        ];
        Ok(u64::from_le_bytes(out))
    }

    fn spown_reverse_reader_at(&mut self, offset: usize)-> Result<Self::Rev, ReaderErr>;
}

impl ByteReader for vec::IntoIter<u8> {
    fn read_u8(&mut self) -> Result<u8, ReaderErr> {
        self.next().ok_or(ReaderErr::NotEnoughData)
    }

    fn read_u16(&mut self) -> Result<u16, ReaderErr> {
        let out = [
            self.next().ok_or(ReaderErr::NotEnoughData)?,
            self.next().ok_or(ReaderErr::NotEnoughData)?
        ];
        Ok(u16::from_le_bytes(out))
    }

    fn read_u32(&mut self) -> Result<u32, ReaderErr> {
        let out = [
            self.next().ok_or(ReaderErr::NotEnoughData)?,
            self.next().ok_or(ReaderErr::NotEnoughData)?,
            self.next().ok_or(ReaderErr::NotEnoughData)?,
            self.next().ok_or(ReaderErr::NotEnoughData)?
        ];
        Ok(u32::from_le_bytes(out))
    }

    fn read_u64(&mut self) -> Result<u64, ReaderErr> {
        let out = [
            self.next().ok_or(ReaderErr::NotEnoughData)?,
            self.next().ok_or(ReaderErr::NotEnoughData)?,
            self.next().ok_or(ReaderErr::NotEnoughData)?,
            self.next().ok_or(ReaderErr::NotEnoughData)?,
            self.next().ok_or(ReaderErr::NotEnoughData)?,
            self.next().ok_or(ReaderErr::NotEnoughData)?,
            self.next().ok_or(ReaderErr::NotEnoughData)?,
            self.next().ok_or(ReaderErr::NotEnoughData)?
        ];
        Ok(u64::from_le_bytes(out))
    }

    type Rev = Rev<vec::IntoIter<u8>>;
    fn spown_reverse_reader_at(&mut self, offset: usize) -> Result<Self::Rev, ReaderErr> {
        let mut vec: Vec<_> = self.collect();
        if offset > vec.len() {
            return Err(ReaderErr::NotEnoughData);
        }
        let rest = vec.split_off(offset);
        let rev = vec.into_iter().rev();
        *self = rest.into_iter();
        Ok(rev)
    }
}


pub struct FunctionalByteReader<R> {
    read_fn: R,
}

impl<R: FnMut()->Result<u8, ReaderErr>> ByteReader for FunctionalByteReader<R> {
    fn read_u8(&mut self) -> Result<u8, ReaderErr> {
        (self.read_fn)()
    }

    fn read_u16(&mut self) -> Result<u16, ReaderErr> {
        let out = [
            self.read_u8()?,
            self.read_u8()?
        ];
        Ok(u16::from_le_bytes(out))
    }

    fn read_u32(&mut self) -> Result<u32, ReaderErr> {
        let out = [
            self.read_u8()?,
            self.read_u8()?,
            self.read_u8()?,
            self.read_u8()?
        ];
        Ok(u32::from_le_bytes(out))
    }

    fn read_u64(&mut self) -> Result<u64, ReaderErr> {
        let out = [
            self.read_u8()?,
            self.read_u8()?,
            self.read_u8()?,
            self.read_u8()?,
            self.read_u8()?,
            self.read_u8()?,
            self.read_u8()?,
            self.read_u8()?
        ];
        Ok(u64::from_le_bytes(out))
    }

    type Rev = Rev<vec::IntoIter<u8>>;

    fn spown_reverse_reader_at(&mut self, offset: usize)-> Result<Self::Rev, ReaderErr> {
        let mut vec = Vec::new();
        for _ in 0..offset {
            vec.push(self.read_u8()?);
        }
        let rest = vec.into_iter().rev();
        Ok(rest)
    }
}

impl<R: FnMut() -> Result<u8, ReaderErr>> FunctionalByteReader<R> {
    pub fn new(read_fn: R) -> Self {
        FunctionalByteReader { read_fn }
    }
}

pub struct BitReader<'buffer, Buffer, Order: OrderConfig = MsbFirst> {
    buffer: &'buffer mut Buffer,

    /// The position in the current byte where the next bit will be read.
    /// It is always less than 8.
    pos_in_curr_byte: u8,

    /// The current byte being read from. This byte will be read from the buffer when it is full.
    curr_byte: u8,

    phantom: std::marker::PhantomData<Order>,
}

impl<'buffer, Buffer: ByteReader, Order: OrderConfig> BitReader<'buffer, Buffer, Order> {
    /// Spowns a new BitReader from the given buffer if the buffer is not empty.
    /// If the buffer is empty then it returns 'None'.
    pub fn spown_from(buffer: &'buffer mut Buffer) -> Option<BitReader<'buffer, Buffer, Order>> {
        Some(
            BitReader {
                buffer,
                pos_in_curr_byte: 0,
                curr_byte: 0,
                phantom: std::marker::PhantomData,
            }
        )
    }

    /// Reads 'size' bits from the buffer and returns them as a 'u64'.
    /// 'size' must be greater than 0 and less than or equal to 64.
    pub fn read_bits(&mut self, size: u8) -> Result<u64, ReaderErr> {
        debug_assert!(size > 0 && size <= 64, "Size must be between 1 and 64 bits.");

		let mut offset = if Order::IS_MSB_FIRST{ size } else { 0 };
		let mut value: u64 = 0;
		
		if self.pos_in_curr_byte != 0 {
            // Safety: by definition, `pos_in_curr_byte` is always less than 8.
			let num_remaining_in_curr_byte = unsafe{ 8_u8.unchecked_sub(self.pos_in_curr_byte) };
			if size <= num_remaining_in_curr_byte { 
				value = unsafe {
					if Order::IS_MSB_FIRST {
						(self.curr_byte & ((1<<num_remaining_in_curr_byte)-1)) >> (num_remaining_in_curr_byte.unchecked_sub(size))
					} else {
						self.curr_byte >> self.pos_in_curr_byte
					}
				} as u64;
				self.pos_in_curr_byte = if size == num_remaining_in_curr_byte {
					0
				} else {
					unsafe { self.pos_in_curr_byte.unchecked_add(size) }
				};
				return Ok( value&((1<<size)-1) );
			}
			value = if Order::IS_MSB_FIRST {
                ((self.curr_byte as usize) & ((1<<num_remaining_in_curr_byte)-1)) << unsafe { size.unchecked_sub(num_remaining_in_curr_byte) }
            } else {
                (self.curr_byte >> self.pos_in_curr_byte) as usize
			} as u64;
			offset = if Order::IS_MSB_FIRST {
				unsafe{ offset.unchecked_sub(num_remaining_in_curr_byte) }
			} else {
				num_remaining_in_curr_byte
			};
		}
		
		
		for _ in 0..if Order::IS_MSB_FIRST{ offset } else { unsafe{ size.unchecked_sub(offset) } } >> 3 {
            self.curr_byte = self.buffer.read_u8()?;
			if Order::IS_MSB_FIRST {
				offset = unsafe{ offset.unchecked_sub(8) };
			}
			value |= (self.curr_byte as u64) << offset;
			if !Order::IS_MSB_FIRST {
				offset = unsafe{ offset.unchecked_add(8) };
			}
		}

		// 'size - offset' is the number of bits remaining to be read.
        if (Order::IS_MSB_FIRST&&offset>0) || (!Order::IS_MSB_FIRST&&size - offset > 0) {
            self.curr_byte = self.buffer.read_u8()?;
            value |= unsafe {
                if Order::IS_MSB_FIRST {
                    (self.curr_byte as u64 >> (8_u8.unchecked_sub(offset))) & ((1<<offset)-1)
                } else {
                    (self.curr_byte as u64 & ((1<<size.unchecked_sub(offset))-1)) << offset
                }
            };
        }

		self.pos_in_curr_byte = if Order::IS_MSB_FIRST {
			offset
		} else {
			(size-offset) & 7
		};
		Ok( value )
    }
}


#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderErr {
    #[error("Not enough data to read")]
    NotEnoughData,
}


pub trait ReverseByteReader {
    fn read_u8_back(&mut self) -> Result<u8, ReaderErr>;
    fn read_u16_back(&mut self) -> Result<u16, ReaderErr> {
        let mut out = [
            self.read_u8_back()?,
            self.read_u8_back()?
        ];
        out.reverse();
        Ok(u16::from_le_bytes(out))
    }
    fn read_u24_back(&mut self) -> Result<u32, ReaderErr> {
        let mut out = [
            self.read_u8_back()?,
            self.read_u8_back()?,
            self.read_u8_back()?
        ];
        out.reverse();
        Ok(u32::from_le_bytes([out[0], out[1], out[2], 0]))
    }
    fn read_u32_back(&mut self) -> Result<u32, ReaderErr> {
        let mut out = [
            self.read_u8_back()?,
            self.read_u8_back()?,
            self.read_u8_back()?,
            self.read_u8_back()?
        ];
        out.reverse();
        Ok(u32::from_le_bytes(out))
    }
    fn read_u64_back(&mut self) -> Result<u64, ReaderErr> {
        let mut out = [
            self.read_u8_back()?,
            self.read_u8_back()?,
            self.read_u8_back()?,
            self.read_u8_back()?,
            self.read_u8_back()?,
            self.read_u8_back()?,
            self.read_u8_back()?,
            self.read_u8_back()?
        ];
        out.reverse();
        Ok(u64::from_le_bytes(out))
    }
}

impl<I: DoubleEndedIterator<Item = u8>> ReverseByteReader for Rev<I> {
    fn read_u8_back(&mut self) -> Result<u8, ReaderErr> {
        self.next().ok_or(ReaderErr::NotEnoughData)
    }
}


#[cfg(test)]
mod tests {
    use std::io::Write;

    use crate::core::{bit_coder::BitReader, buffer::*};
    use crate::prelude::ByteWriter;
    use super::BitWriter;

    #[test]
    fn test_writer_reader_msb_first() {
        let mut buffer = Vec::new();
        let mut writer = BitWriter::<_,MsbFirst>::spown_from(&mut buffer);
        writer.write_bits((2, 0b10));
        writer.write_bits((3, 0b011));
        drop(writer); // drop the bit writer to end bit-writing
        assert_eq!(buffer.len(), 1);
        let mut reader = buffer.into_iter();
        let mut reader = BitReader::<_,MsbFirst>::spown_from(&mut reader).unwrap();
        assert_eq!(reader.read_bits(2).unwrap(), 0b10);
        assert_eq!(reader.read_bits(3).unwrap(), 0b011);

        let mut buffer = Vec::new();
        let mut writer = BitWriter::<_,MsbFirst>::spown_from(&mut buffer);
        writer.write_bits((7, 0b0111010));
        drop(writer); // drop the bit writer to end bit-writing
        assert_eq!(buffer.len(), 1);
        let mut reader = buffer.into_iter();
        let mut reader = BitReader::<_,MsbFirst>::spown_from(&mut reader).unwrap();
        assert_eq!(reader.read_bits(7).unwrap(), 0b0111010);

        let mut buffer = Vec::new();
        let mut writer: BitWriter<_> = BitWriter::spown_from(&mut buffer);
        writer.write_bits((8, 0b10111010));
        drop(writer); // drop the bit writer to end bit-writing
        assert_eq!(buffer[0], 0b10111010);
        let mut reader = buffer.into_iter();
        let mut reader: BitReader<_> = BitReader::spown_from(&mut reader).unwrap();
        assert_eq!(reader.read_bits(8).unwrap(), 0b10111010);

        let mut buffer = Vec::new();
        let mut writer: BitWriter<_> = BitWriter::spown_from(&mut buffer);
        writer.write_bits((9, 0b110111011));
        drop(writer); // drop the bit writer to end bit-writing
        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer[0], 0b11011101);
        assert_eq!(buffer[1], 0b10000000);
        let mut reader = buffer.into_iter();
        let mut reader: BitReader<_> = BitReader::spown_from(&mut reader).unwrap();
        assert_eq!(reader.read_bits(9).unwrap(), 0b110111011);
        
        let mut buffer = Vec::new();
        let mut writer: BitWriter<_> = BitWriter::spown_from(&mut buffer);
        writer.write_bits((9, 0b101010100));
        writer.write_bits((8, 0b10101110));
        writer.write_bits((7, 0b0101010));
        writer.write_bits((6, 0b111100));
        writer.write_bits((5, 0b00001));
        writer.write_bits((4, 0b1100));
        drop(writer); // drop the bit writer to end bit-writing
        assert_eq!(buffer.len(), (9+8+7+6+5+4)/8+1);
        assert_eq!(buffer[0], 0b10101010);
        assert_eq!(buffer[1], 0b01010111);
        assert_eq!(buffer[2], 0b00101010);
        assert_eq!(buffer[3], 0b11110000);
        assert_eq!(buffer[4], 0b00111000);
        let mut reader = buffer.into_iter();
        let mut reader: BitReader<_> = BitReader::spown_from(&mut reader).unwrap();
        assert_eq!(reader.read_bits(9).unwrap(), 0b101010100);
        assert_eq!(reader.read_bits(8).unwrap(), 0b10101110);
        assert_eq!(reader.read_bits(7).unwrap(), 0b0101010);
        assert_eq!(reader.read_bits(6).unwrap(), 0b111100);
        assert_eq!(reader.read_bits(5).unwrap(), 0b00001);
        assert_eq!(reader.read_bits(4).unwrap(), 0b1100);
        
        let mut buffer = Vec::new();
        let mut writer: BitWriter<_> = BitWriter::spown_from(&mut buffer);
        writer.write_bits((11, 0b10111010110));
        drop(writer); // drop the bit writer to end bit-writing
        assert_eq!(buffer.len(), 2);
        let mut reader = buffer.into_iter();
        let mut reader: BitReader<_> = BitReader::spown_from(&mut reader).unwrap();
        assert_eq!(reader.read_bits(2).unwrap(), 0b10);
        assert_eq!(reader.read_bits(1).unwrap(), 0b1);
        assert_eq!(reader.read_bits(3).unwrap(), 0b110);
        assert_eq!(reader.read_bits(3).unwrap(), 0b101);
        assert_eq!(reader.read_bits(2).unwrap(), 0b10);

    }

    #[test]
    fn test_writer_reader_lsb_first() {
        let mut buffer = Vec::new();
        {
            let mut writer = BitWriter::<_,LsbFirst>::spown_from(&mut buffer);
            writer.write_bits((9, 0b101010100));
            writer.write_bits((8, 0b10101010));
            writer.write_bits((7, 0b0101010));
            writer.write_bits((6, 0b111100));
            writer.write_bits((5, 0b00001));
            writer.write_bits((4, 0b1100));
        }
        assert_eq!(buffer.len(), (9+8+7+6+5+4)/8+1);
        let mut reader = buffer.into_iter();
        let mut reader = BitReader::<_,LsbFirst>::spown_from(&mut reader).unwrap();
        assert_eq!(reader.read_bits(9).unwrap(), 0b101010100);
        assert_eq!(reader.read_bits(8).unwrap(), 0b10101010);
        assert_eq!(reader.read_bits(7).unwrap(), 0b0101010);
        assert_eq!(reader.read_bits(6).unwrap(), 0b111100);
        assert_eq!(reader.read_bits(5).unwrap(), 0b00001);
        assert_eq!(reader.read_bits(4).unwrap(), 0b1100);

        let mut buffer = Vec::new();
        {
            let mut writer = BitWriter::<_,LsbFirst>::spown_from(&mut buffer);
            writer.write_bits((10, 0b1010101010));
        }
        assert_eq!(buffer.len(), 2);
        let mut reader = buffer.into_iter();
        let mut reader = BitReader::<_,LsbFirst>::spown_from(&mut reader).unwrap();
        for _ in 0..5 {
            assert_eq!(reader.read_bits(2).unwrap(), 0b10);
        }
    }

    use crate::core::bit_coder::ByteReader;
    use crate::core::bit_coder::ReverseByteReader;
    use crate::core::bit_coder::ReaderErr::NotEnoughData;
    #[test]
    fn test_reverse_reader1() {
        let buffer = vec![1_u8, 2, 3, 4, 5];
        let mut reader = buffer.into_iter();
        let mut reverse_reader = reader.spown_reverse_reader_at(2).unwrap();
        assert_eq!(reverse_reader.read_u8_back().unwrap(), 2);
        assert_eq!(reverse_reader.read_u8_back().unwrap(), 1);
        assert_eq!(reverse_reader.read_u8_back(), Err(NotEnoughData));
        assert!(reader.next().unwrap() == 3);
        assert!(reader.next().unwrap() == 4);
        assert!(reader.next().unwrap() == 5);
        assert!(reader.next().is_none());
    }

        #[test]
    fn test_reverse_reader2() {
        let mut buffer = Vec::new();
        buffer.write_u8(200);
        buffer.write_u16(201);
        buffer.write_u24(202);
        buffer.write_u32(203);
        assert!(buffer.len() == 10);
        let mut reader = buffer.into_iter();
        let mut reverse_reader = reader.spown_reverse_reader_at(10).unwrap();
        assert_eq!(reverse_reader.read_u32_back().unwrap(), 203);
        assert_eq!(reverse_reader.read_u24_back().unwrap(), 202);
        assert_eq!(reverse_reader.read_u16_back().unwrap(), 201);
        assert_eq!(reverse_reader.read_u8_back().unwrap(), 200);
        assert_eq!(reverse_reader.read_u8_back(), Err(NotEnoughData));
        assert!(reader.next().is_none());
    }
}