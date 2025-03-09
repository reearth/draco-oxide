use crate::core::{buffer::{reader::Reader, writer::Writer, MSB_FIRST}, shared::ConfigType};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Symbol {
    C,
    R,
    L,
    E,
    S,
    M,
}

#[derive(Clone, Copy)]
pub enum SymbolEncodingConf {
	/// The default binary representations for the CLERS symbols, defined
	/// as follows:
	/// C: 0
	/// R: 10
	/// L: 1100
	/// E: 1101
	/// S: 1110
	/// M: 1111
	CrLight,
	
	/// Another choice for the binary representations for the CLERS symbols, defined
	/// as follows:
	/// C: 0
	/// R: 100
	/// L: 110
	/// E: 101
	/// S: 1110
	/// M: 1111
	Balanced,
	
	Rans,
}

impl ConfigType for SymbolEncodingConf {
    fn default() -> Self {
        SymbolEncodingConf::CrLight
    }
}

impl SymbolEncodingConf {
    pub(crate) fn write_symbol_encoding(writer: &mut Writer<MSB_FIRST>, conf: SymbolEncodingConf) {
        let id = match conf {
            SymbolEncodingConf::CrLight => 0,
            SymbolEncodingConf::Balanced => 1,
            SymbolEncodingConf::Rans => 2,
        };
        writer.next((2, id));
    }

    pub(crate) fn get_symbol_encoding(reader: &mut Reader) -> SymbolEncodingConf {
        match reader.next(2) {
            0 => SymbolEncodingConf::CrLight,
            1 => SymbolEncodingConf::Balanced,
            2 => SymbolEncodingConf::Rans,
            _ => panic!("Intenal Error: Invalid symbol encoding configuration")
        }
    }
}

pub(super) trait SymbolEncoder {
    fn encode_symbol(symbol: Symbol) -> (usize, usize);
    fn decode_symbol(reader: &mut Reader) -> Symbol;
}

pub(crate) struct CrLight;
impl SymbolEncoder for CrLight {
    fn encode_symbol(symbol: Symbol) -> (usize, usize) {
        match symbol {
            Symbol::C => (1, 0),
            Symbol::R => (2, 0b10),
            Symbol::L => (4, 0b1100),
            Symbol::E => (4, 0b1101),
            Symbol::S => (4, 0b1110),
            Symbol::M => (4, 0b1111)
        }
    }

    fn decode_symbol(reader: &mut Reader) -> Symbol {
        if reader.next(1) == 0 {
            return Symbol::C;
        }

        if reader.next(1) == 0 {
            return Symbol::R;
        }

        return match reader.next(2) {
            0b00 => Symbol::L,
            0b01 => Symbol::E,
            0b10 => Symbol::S,
            0b11 => Symbol::M,
            _ => Symbol::M
        }
    }
}

pub(super) struct Balanced;

impl SymbolEncoder for Balanced {
    fn encode_symbol(symbol: Symbol) -> (usize, usize) {
        match symbol {
            Symbol::C => (1, 0),
            Symbol::R => (3, 0b100),
            Symbol::L => (3, 0b110),
            Symbol::E => (3, 0b101),
            Symbol::S => (4, 0b1110),
            Symbol::M => (4, 0b1111)
        }
    }

    fn decode_symbol(reader: &mut Reader) -> Symbol {
        Symbol::C
    }
}

pub(super) struct Rans {
    distribution: Vec<usize>,
    mask: usize,
}

impl SymbolEncoder for Rans {
    fn encode_symbol(symbol: Symbol) -> (usize, usize) {
        match symbol {
            Symbol::C => (1, 0),
            Symbol::R => (2, 0b01),
            Symbol::L => (4, 0b0011),
            Symbol::E => (4, 0b1011),
            Symbol::S => (4, 0b0111),
            Symbol::M => (4, 0b1111)
        }
    }

    fn decode_symbol(reader: &mut Reader) -> Symbol {
        Symbol::C
    }
}

