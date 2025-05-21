use crate::core::shared::ConfigType;
use crate::encode::connectivity::edgebreaker::Err;

use super::{HANDLE_METADATA_SLOTS, NUM_VERTICES_IN_HOLE_SLOTS, SYMBOL_ENCODING_CONFIG_SLOT};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Symbol {
    C,
    R,
    L,
    E,
    S,
    M(usize), // Nmbber of vertices in the hole.
    H(usize), // Number of vertices in the handle.
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
            Symbol::M(n_vertices) => ('M', Some(*n_vertices)),
            Symbol::H(n_vertices) => ('H', Some(*n_vertices)),
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
	/// M: 11110
    /// H: 11111
	CrLight,
	
	/// Another choice for the binary representations for the CLERS symbols, defined
	/// as follows:
	/// C: 0
	/// R: 100
	/// L: 110
	/// E: 101
	/// S: 11100
	/// M: 11101
    /// H: 11110
	Balanced,
	
	Rans,
}

impl ConfigType for SymbolEncodingConfig {
    fn default() -> Self {
        Self::CrLight
    }
}

impl SymbolEncodingConfig {
    pub(crate) fn write_symbol_encoding<F>(self, writer: &mut F) 
        where F: FnMut((u8, u64))
    {
        let id = match self {
            Self::CrLight => 0,
            Self::Balanced => 1,
            Self::Rans => 2,
        };
        writer((SYMBOL_ENCODING_CONFIG_SLOT, id));
    }

    pub(crate) fn get_symbol_encoding<F>(reader: &mut F) -> Self 
        where F: FnMut(u8) -> u64
    {
        match reader(SYMBOL_ENCODING_CONFIG_SLOT) {
            0 => Self::CrLight,
            1 => Self::Balanced,
            2 => Self::Rans,
            _ => panic!("Internal Error: Invalid symbol encoding configuration")
        }
    }
}
pub(crate) trait SymbolEncoder {
    fn encode_symbol(symbol: Symbol) -> Result<(u8, u64), Err>;
    fn decode_symbol<F>(reader: &mut F) -> Symbol
        where F: FnMut(u8) -> u64;
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
            Symbol::M(n_vertices) => {
                let size = if n_vertices >> 8 == 0 {
                    0
                } else if n_vertices >> 12 == 0 {
                    1
                } else if n_vertices >> 16 == 0 {
                    2
                } else if n_vertices >> 20 == 0 {
                    3
                } else {
                    return Err(Err::HoleSizeTooLarge);
                };
                let slot_size = NUM_VERTICES_IN_HOLE_SLOTS[size];
                Ok((
                    (5 + 2 + slot_size) as u8, 
                    (0b11110 << (2+slot_size) | size << slot_size | n_vertices) as u64
                ))
            },
            Symbol::H(metadata) => {
                let size = if metadata >> 8 == 0 {
                    0
                } else if metadata >> 12 == 0 {
                    1
                } else if metadata >> 16 == 0 {
                    2
                } else if metadata >> 20 == 0 {
                    3
                } else {
                    return Err(Err::HandleSizeTooLarge);
                };
                let slot_size = HANDLE_METADATA_SLOTS[size];
                Ok((
                    (5 + 2 + slot_size) as u8,
                    (0b11111 << (2+slot_size) | size << slot_size | metadata) as u64
                ))
            },
        }
    }

    fn decode_symbol<F>(reader: &mut F) -> Symbol 
        where F: FnMut(u8) -> u64
    {
        if reader(1) == 0 {
            return Symbol::C;
        }

        if reader(1) == 0 {
            return Symbol::R;
        }

        match reader(2) {
            0b00 => return Symbol::L,
            0b01 => return Symbol::E,
            0b10 => return Symbol::S,
            _ => {}
        }

        return match reader(1) {
            0 => {
                // M
                let size = reader(2);
                let n_vertices = reader(NUM_VERTICES_IN_HOLE_SLOTS[size as usize]);
                Symbol::M(n_vertices as usize)
            },
            1 => {
                // H
                let size = reader(2);
                let n_vertices = reader(HANDLE_METADATA_SLOTS[size as usize]);
                Symbol::H(n_vertices as usize)
            },
            _ => unreachable!("Interanl Error: There must be a bug in the buffer implementation.")
        }
    }
}

pub(crate) struct Balanced;

impl SymbolEncoder for Balanced {
    fn encode_symbol(symbol: Symbol) -> Result<(u8, u64), Err> {
        match symbol {
            Symbol::C => Ok((1, 0)),
            Symbol::R => Ok((3, 0b100)),
            Symbol::L => Ok((3, 0b110)),
            Symbol::E => Ok((3, 0b101)),
            Symbol::S => Ok((5, 0b11100)),
            Symbol::M(n_vertices) => {
                let size = if n_vertices >> 8 == 0 {
                    0
                } else if n_vertices >> 12 == 0 {
                    1
                } else if n_vertices >> 16 == 0 {
                    2
                } else if n_vertices >> 20 == 0 {
                    3
                } else {
                    return Err(Err::HoleSizeTooLarge);
                };
                let slot_size = NUM_VERTICES_IN_HOLE_SLOTS[size];
                Ok((
                    (5 + 2 + slot_size) as u8, 
                    (0b11101 << (2+slot_size) | size << slot_size | n_vertices) as u64
                ))
            },
            Symbol::H(n_vertices) => {
                let size = if n_vertices >> 8 == 0 {
                    0
                } else if n_vertices >> 12 == 0 {
                    1
                } else if n_vertices >> 16 == 0 {
                    2
                } else if n_vertices >> 20 == 0 {
                    3
                } else {
                    return Err(Err::HandleSizeTooLarge);
                };
                let slot_size = HANDLE_METADATA_SLOTS[size];
                Ok((
                    (5 + 2 + slot_size) as u8,
                    (0b11110 << (2+slot_size) | size << slot_size | n_vertices) as u64
                ))
            },
        }
    }

    fn decode_symbol<F>(reader: &mut F) -> Symbol 
        where F: FnMut(u8) -> u64
    {
        if reader(1) == 0 {
            return Symbol::C;
        }

        if reader(1) == 0 {
            return Symbol::R;
        }

        match reader(2) {
            0b00 => return Symbol::L,
            0b01 => return Symbol::E,
            0b10 => return Symbol::S,
            _ => {}
        }

        return match reader(1) {
            0 => {
                // M
                let size = reader(2);
                let n_vertices = reader(NUM_VERTICES_IN_HOLE_SLOTS[size as usize]);
                Symbol::M(n_vertices as usize)
            },
            1 => {
                // H
                let size = reader(2);
                let n_vertices = reader(HANDLE_METADATA_SLOTS[size as usize]);
                Symbol::H(n_vertices as usize)
            },
            _ => unreachable!("Interanl Error: There must be a bug in the buffer implementation.")
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

    fn decode_symbol<F>(_reader: &mut F) -> Symbol 
        where F: FnMut(u8) -> u64
    {
        unimplemented!()
    }
}

