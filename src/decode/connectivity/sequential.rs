use crate::shared::connectivity::sequential::{index_size_from_vertex_count, NUM_POINTS_SLOT};
use crate::core::shared::VertexIdx;
use super::ConnectivityDecoder;

pub(crate) struct Sequential;


impl ConnectivityDecoder for Sequential {

    fn decode_connectivity(&mut self, mut reader: crate::core::buffer::reader::Reader) -> Vec<[VertexIdx; 3]> {
        let num_points = reader.next(NUM_POINTS_SLOT);
        let num_faces = reader.next(NUM_POINTS_SLOT);
        
        let index_size = index_size_from_vertex_count(num_points).unwrap();
        

        let faces = (0..num_faces).map(|_| {
            [
                reader.next(index_size) as VertexIdx,
                reader.next(index_size) as VertexIdx,
                reader.next(index_size) as VertexIdx,
            ]
        }).collect();
            
        faces
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::buffer::writer::Writer;
    use crate::core::buffer;
    use crate::encode::connectivity::ConnectivityEncoder;
    use crate::encode;
    use crate::encode::connectivity::sequential::Config;
    use crate::core::shared::ConfigType;


    #[test]
    fn test_encode_connectivity() {
        let mut encoder = encode::connectivity::sequential::Sequential;
        let mut buffer = Writer::new();
        let mut faces = vec![
        let faces = vec![
            [9,12,13], [8,9,13], [8,9,10], [1,8,10], [1,10,11], [1,2,11], [2,11,12], [2,12,13],
            [8,13,14], [7,8,14], [1,7,8], [0,1,7], [0,1,2], [0,2,3], [2,3,13], [3,13,14],
            [7,14,15], [6,7,15], [0,6,7], [0,5,6], [0,3,5], [3,4,5], [3,4,14], [4,14,15],
            [6,12,15], [6,9,12], [5,6,9], [5,9,10], [4,5,10], [4,10,11], [4,11,15], [11,12,15]
        ];
        let mut points = [[0.0, 0.0, 0.0]; 9];
        let result = encoder.encode_connectivity(&mut faces, &Config::default(), &mut points, &mut buffer);
        assert!(result.is_ok());
        let buffer: buffer::Buffer = buffer.into();
        let reader = buffer.into_reader();
        let mut decoder = Sequential;
        let decoded_faces = decoder.decode_connectivity(reader);
        assert_eq!(faces, decoded_faces);
    }
}