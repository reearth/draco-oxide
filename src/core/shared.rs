pub type VertexIdx = usize;
pub type EdgeIdx = usize;
pub type FaceIdx = usize;


pub trait Float {
    type Bits;
    fn from_bits(bits: Self::Bits)-> Self;
    fn to_bits(self)-> Self::Bits;
}

impl Float for f32 {
    type Bits = u32;
    fn from_bits(bits: Self::Bits)-> Self {
        Self::from_bits(bits)
    }
    fn to_bits(self)-> Self::Bits {
        self.to_bits()
    }
}

impl Float for f64 {
    type Bits = u64;
    fn from_bits(bits: Self::Bits)-> Self {
        Self::from_bits(bits)
    }
    fn to_bits(self)-> Self::Bits {
        self.to_bits()
    }
}

pub trait ConfigType {
    fn default()-> Self;
}