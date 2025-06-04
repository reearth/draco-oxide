use crate::core::bit_coder::ReaderErr;
use crate::debug_expect;
use crate::prelude::ByteReader;
use crate::shared::connectivity::sequential::{index_size_from_vertex_count, Method};
use crate::core::shared::VertexIdx;
use crate::utils::bit_coder::leb128_read;
use super::ConnectivityDecoder;

#[derive(thiserror::Error, Debug)]
#[remain::sorted]
pub enum Err {
    #[error("Stream input returned with None, though more data was expected.")]
    NotEnoughData(#[from] ReaderErr),
}

pub(crate) struct Sequential;


impl ConnectivityDecoder for Sequential {
    fn decode_connectivity<R>(&mut self, reader: &mut R) -> Result<Vec<[VertexIdx; 3]>, super::Err> 
        where R: ByteReader
    {
        let num_points = reader.read_u64()?;
        let num_faces = reader.read_u64()?;

        let _encoder_method = Method::from_id(
            reader.read_u8()?
        );
        
        let index_size = index_size_from_vertex_count(num_points as usize).unwrap() as u8;

        debug_expect!("Start of indices", reader);
        let faces = if index_size == 21 {
            // varint decoding
            (0..num_faces).map(|_| {
                [
                    leb128_read(reader).unwrap() as VertexIdx, // ToDo: handle errors properly
                    leb128_read(reader).unwrap() as VertexIdx,
                    leb128_read(reader).unwrap() as VertexIdx
                ]
            }).collect()
        } else {
            // non-varint decoding
            match index_size {
                8 => (0..num_faces).map(|_| {
                        [
                            // ToDo: Avoid unwraps and handle errors with 'Err::NotEnoughData'
                            reader.read_u8().unwrap() as VertexIdx,
                            reader.read_u8().unwrap() as VertexIdx,
                            reader.read_u8().unwrap() as VertexIdx,
                        ]
                    }).collect(),
                16 => (0..num_faces).map(|_| {
                        [
                            reader.read_u16().unwrap() as VertexIdx,
                            reader.read_u16().unwrap() as VertexIdx,
                            reader.read_u16().unwrap() as VertexIdx,
                        ]
                    }).collect(),
                32 => (0..num_faces).map(|_| {
                        [
                            reader.read_u32().unwrap() as VertexIdx,
                            reader.read_u32().unwrap() as VertexIdx,
                            reader.read_u32().unwrap() as VertexIdx,
                        ]
                    }).collect(),
                _ => unreachable!()
            }
        };
        Ok(faces)
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::attribute::AttributeId;
    use crate::encode::connectivity::ConnectivityEncoder;
    use crate::encode;
    use crate::core::shared::{NdVector, Vector};
    use crate::prelude::{Attribute, AttributeType};
    use crate::shared::connectivity::sequential;


    #[test]
    fn test_encode_connectivity() {
        let mut encoder = encode::connectivity::sequential::Sequential::new(
            encode::connectivity::sequential::Config {
                encoder_method: sequential::Method::DirectIndices,
            }
        );
        let mut writer = Vec::new();
        let mut faces = vec![
            [9,12,13], [8,9,13], [8,9,10], [1,8,10], [1,10,11], [1,2,11], [2,11,12], [2,12,13],
            [8,13,14], [7,8,14], [1,7,8], [0,1,7], [0,1,2], [0,2,3], [2,3,13], [3,13,14],
            [7,14,15], [6,7,15], [0,6,7], [0,5,6], [0,3,5], [3,4,5], [3,4,14], [4,14,15],
            [6,12,15], [6,9,12], [5,6,9], [5,9,10], [4,5,10], [4,10,11], [4,11,15], [11,12,15]
        ];
        let points = vec![NdVector::<3,f32>::zero(); 9];
        let mut point_att = Attribute::from(AttributeId::new(0), points, AttributeType::Position, Vec::new());
        let result = encoder.encode_connectivity(&mut faces, &mut [&mut point_att], &mut writer);
        assert!(result.is_ok());
        let mut reader = writer.into_iter();
        let mut decoder = Sequential;
        let decoded_faces = decoder.decode_connectivity(&mut reader);
        assert_eq!(faces, decoded_faces.unwrap());
    }
}