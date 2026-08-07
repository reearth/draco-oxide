#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Symbol {
    C,
    S,
    L,
    R,
    E,
}

impl Symbol {
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
}
