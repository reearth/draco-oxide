use crate::core::{bit_coder::ByteWriter, shared::ConfigType};
use crate::encode::connectivity::edgebreaker::Err;
use crate::prelude::{BitReader, ByteReader};

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

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SymbolEncodingConfig {
	/// The default binary representations for the CLERS symbols, defined
	/// as follows:
	/// C: 0
	/// R: 10
	/// L: 1100
	/// E: 1101
	/// S: 1110
	CrLight,
	
	Rans,
}

impl ConfigType for SymbolEncodingConfig {
    fn default() -> Self {
        Self::CrLight
    }
}

impl SymbolEncodingConfig {
    pub(crate) fn write_symbol_encoding<W>(self, writer: &mut W) 
        where W: ByteWriter
    {
        let id = match self {
            Self::CrLight => 0,
            Self::Rans => 1,
        };
        writer.write_u8(id);
    }

    pub(crate) fn get_symbol_encoding<R>(reader: &mut R) -> Self 
        where R: ByteReader
    {
        match reader.read_u8().unwrap() { // TODO: handle error properly
            0 => Self::CrLight,
            1 => Self::Rans,
            _ => panic!("Internal Error: Invalid symbol encoding configuration")
        }
    }
}
pub(crate) trait SymbolEncoder {
    fn encode_symbol(symbol: Symbol) -> Result<(u8, u64), Err>;
    fn decode_symbol<R>(reader: &mut BitReader<R>) -> Symbol where R: ByteReader;
}

pub(crate) struct CrLight;
impl SymbolEncoder for CrLight {
    fn encode_symbol(symbol: Symbol) -> Result<(u8, u64), Err> {
        match symbol {
            Symbol::C => Ok((1, 0)),
            Symbol::R => Ok((2, 0b10)),
            Symbol::L => Ok((4, 0b1100)),
            Symbol::E => Ok((4, 0b1101)),
            Symbol::S => Ok((4, 0b1110)),
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

pub(crate) struct Rans {
    distribution: Vec<usize>,
    mask: usize,
}

impl SymbolEncoder for Rans {
    fn encode_symbol(_symbol: Symbol) -> Result<(u8, u64), Err> {
        unimplemented!()
    }

    fn decode_symbol<R>(_reader: &mut BitReader<R>) -> Symbol 
        where R: ByteReader
    {
        unimplemented!()
    }
}

