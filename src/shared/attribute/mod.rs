pub mod prediction_scheme;
pub mod portabilization;

pub trait Portable {
    fn to_bits(&self) -> Vec<(u8, u64)>;
    fn read_from_bits<F>(stream_in: &mut F) -> Self
        where F: FnMut(u8) -> u64;
}


impl Portable for bool {
    fn to_bits(&self) -> Vec<(u8, u64)> {
        vec![(1, *self as u64)]
    }
    fn read_from_bits<F>(stream_in: &mut F) -> Self
        where F: FnMut(u8) -> u64
    {
        stream_in(1) != 0
    }
}


#[cfg(test)]
mod tests {
    use crate::prelude::NdVector;
    use crate::core::buffer;
    use super::Portable;

    #[test]
    fn from_bits_f32() {
        let data = NdVector::from([1_f32, -1.0, 1.0]);
        let mut buff_writer = buffer::writer::Writer::new();
        let mut writer = |input| buff_writer.next(input);
        data.to_bits().into_iter().for_each(|w| writer(w));
        let buffer: buffer::Buffer = buff_writer.into();
        let mut buff_reader = buffer.into_reader();
        let mut reader = |size| buff_reader.next(size);
        let dequant_data: NdVector<3,f32> = NdVector::read_from_bits(&mut reader);
        assert_eq!(data, dequant_data);
    }

    #[test]
    fn from_bits_f64() {
        let data = NdVector::from([1_f64, -1.0, 1.0]);
        let mut buff_writer = buffer::writer::Writer::new();
        let mut writer = |input| buff_writer.next(input);
        data.to_bits().into_iter().for_each(|w| writer(w));
        let buffer: buffer::Buffer = buff_writer.into();
        let mut buff_reader = buffer.into_reader();
        let mut reader = |size| buff_reader.next(size);
        let dequant_data: NdVector<3,f64> = NdVector::read_from_bits(&mut reader);
        assert_eq!(data, dequant_data);
    }
}