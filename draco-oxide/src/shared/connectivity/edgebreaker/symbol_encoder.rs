use crate::core::bit_coder::BitReader;
use crate::encode::connectivity::edgebreaker::Err;
use crate::prelude::ByteReader;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Symbol {
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
    pub(crate) fn as_char(&self) -> (char, Option<usize>) {
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
    pub(crate) fn get_id(self) -> usize {
        match self {
            Symbol::C => 0,
            Symbol::S => 1,
            Symbol::L => 2,
            Symbol::R => 3,
            Symbol::E => 4,
        }
    }
}

pub(crate) trait SymbolEncoder {
    fn encode_symbol(symbol: Symbol) -> Result<(u8, u64), Err>;

    #[allow(dead_code)] // TODO: remove this after completing the decoder.
    fn decode_symbol<R>(reader: &mut BitReader<R>) -> Symbol where R: ByteReader;
}

pub(crate) struct CrLight;
impl SymbolEncoder for CrLight {
    fn encode_symbol(symbol: Symbol) -> Result<(u8, u64), Err> {
        match symbol {
            Symbol::C => Ok((1, 0)),
            Symbol::S => Ok((3, 0b1)),
            Symbol::L => Ok((3, 0b11)),
            Symbol::R => Ok((3, 0b101)),
            Symbol::E => Ok((3, 0b111)),
        }
    }

    fn decode_symbol<R>(reader: &mut BitReader<R>) -> Symbol 
        where R: ByteReader
    {
        if reader.read_bits(1).unwrap() == 0 {
            return Symbol::C;
        }

        if reader.read_bits(1).unwrap() == 0 {
            return Symbol::R;
        }

        return match reader.read_bits(2).unwrap() {
            0b00 => Symbol::L,
            0b01 => Symbol::E,
            0b10 => Symbol::S,
            _ => panic!("Internal Error: Invalid symbol encoding"),
        }
    }
}


