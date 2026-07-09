use crate::bit_coder::BitReader;
use crate::bit_coder::ByteReader;
use crate::buffer::OrderConfig;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Symbol {
    C,
    S,
    L,
    R,
    E,
}

impl Symbol {
    #[inline]
    /// Returns the symbol as a character together with the metadata if it is a hole or handle.
    #[allow(unused)] // May be used in the future for debugging or logging.
    pub fn as_char(&self) -> (char, Option<usize>) {
        match self {
            Symbol::C => ('C', None),
            Symbol::R => ('R', None),
            Symbol::L => ('L', None),
            Symbol::E => ('E', None),
            Symbol::S => ('S', None),
        }
    }

    /// Returns the symbol id of the symbol.
    /// This id must be compatible with the draco library.
    pub fn get_id(self) -> usize {
        match self {
            Symbol::C => 0,
            Symbol::S => 1,
            Symbol::L => 2,
            Symbol::R => 3,
            Symbol::E => 4,
        }
    }
}

pub trait SymbolEncoder {
    fn encode_symbol(symbol: Symbol) -> (u8, u64);

    /// Decodes one CrLight-encoded symbol from `reader`. Parameterized over
    /// `OrderConfig` because the encoder writes via `BitWriter<_, LsbFirst>`,
    /// so the matching reader must also use `LsbFirst`.
    fn decode_symbol<R, O>(reader: &mut BitReader<R, O>) -> Symbol
    where
        R: ByteReader,
        O: OrderConfig;
}

pub struct CrLight;
impl SymbolEncoder for CrLight {
    fn encode_symbol(symbol: Symbol) -> (u8, u64) {
        match symbol {
            Symbol::C => (1, 0),
            Symbol::S => (3, 0b1),
            Symbol::L => (3, 0b11),
            Symbol::R => (3, 0b101),
            Symbol::E => (3, 0b111),
        }
    }

    fn decode_symbol<R, O>(reader: &mut BitReader<R, O>) -> Symbol
    where
        R: ByteReader,
        O: OrderConfig,
    {
        // LsbFirst: bit 0 is the lowest-significance bit of `value`. So
        // after reading the leading "1", a `read_bits(2)` returns the
        // next two bits packed as `bit_pos1 << 0 | bit_pos2 << 1`.
        //
        // Mapping from `read_bits(2)` to symbol (consistent with
        // `encode_symbol` LsbFirst values >> 1):
        //   value 0b00 (raw bits 0,0) -> S    (encode value 0b001)
        //   value 0b01 (raw bits 1,0) -> L    (encode value 0b011)
        //   value 0b10 (raw bits 0,1) -> R    (encode value 0b101)
        //   value 0b11 (raw bits 1,1) -> E    (encode value 0b111)
        if reader.read_bits(1).unwrap() == 0 {
            return Symbol::C;
        }
        match reader.read_bits(2).unwrap() {
            0b00 => Symbol::S,
            0b01 => Symbol::L,
            0b10 => Symbol::R,
            0b11 => Symbol::E,
            _ => unreachable!("read_bits(2) returns at most 0b11"),
        }
    }
}
