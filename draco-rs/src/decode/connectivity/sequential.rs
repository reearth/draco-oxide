use crate::shared::connectivity::sequential::{index_size_from_vertex_count, NUM_POINTS_SLOT};
use crate::core::shared::VertexIdx;
use super::ConnectivityDecoder;

#[derive(thiserror::Error, Debug)]
#[remain::sorted]
pub enum Err {
    #[error("Stream input returned with None, though more data was expected.")]
    NotEnoughData,
}

pub(crate) struct Sequential;


impl ConnectivityDecoder for Sequential {
    fn decode_connectivity<F>(&mut self, reader: &mut F) -> Result<Vec<[VertexIdx; 3]>, super::Err> 
        where F: FnMut(u8) -> u64,
    {
        let num_points = reader(NUM_POINTS_SLOT);
        let num_faces = reader(NUM_POINTS_SLOT);
        
        let index_size = index_size_from_vertex_count(num_points as usize).unwrap() as u8;

        let faces = (0..num_faces).map(|_| {
            [
                reader(index_size) as VertexIdx,
                reader(index_size) as VertexIdx,
                reader(index_size) as VertexIdx,
            ]
        }).collect();
            
        Ok(faces)
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::attribute::AttributeId;
    use crate::core::buffer::writer::Writer;
    use crate::core::buffer;
    use crate::encode::connectivity::ConnectivityEncoder;
    use crate::encode;
    use crate::core::shared::{
        NdVector,
        Vector
    };
    use crate::prelude::{Attribute, AttributeType};


    #[test]
    fn test_encode_connectivity() {
        let mut encoder = encode::connectivity::sequential::Sequential;
        let mut buff_writer = Writer::new();
        let mut writer = |input| buff_writer.next(input);
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
        let buffer: buffer::Buffer = buff_writer.into();
        let mut buff_reader = buffer.into_reader();
        let mut reader = |input| buff_reader.next(input);
        let mut decoder = Sequential;
        let decoded_faces = decoder.decode_connectivity(&mut reader);
        assert_eq!(faces, decoded_faces.unwrap());
    }
}