use crate::core::shared::ConfigType;

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

pub(super) trait SymbolEncoder {
    fn encode_symbol(symbol: Symbol) -> (usize, usize);
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
}

pub(super) struct Rans {
    distribution: Vec<usize>,
    mask: usize,
}

impl SymbolEncoder for Rans {
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
}