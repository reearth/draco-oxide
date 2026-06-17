pub(crate) mod attribute_decoder;
pub(crate) mod inverse_prediction_transform;
pub(crate) mod portabilization;

use thiserror::Error;
use crate::{core::bit_coder::ReaderErr, decode::header::Header, prelude::{Attribute, ByteReader, ConfigType}, shared::{attribute::{self, AttributeKind}, header::EncoderMethod}, utils::bit_coder::leb128_read};


#[derive(Debug, Error)]
pub enum Err {
    #[error("Attribute error: {0}")]
    AttributeDecoderError(#[from] attribute_decoder::Err),
    #[error("Prediction inverse transform error: {0}")]
    PredictionInverseTransformError(String),
    #[error("Not Enough Data: {0}")]
    NotEnoughData(#[from] ReaderErr),
    #[error("Attribute Error: {0}")]
    AttributeError(#[from] attribute::Err),
}

#[derive(Debug, Clone)]
pub struct Config {
    decoder_cfgs: Vec<attribute_decoder::Config>,
}

impl ConfigType for Config {
    fn default() -> Self {
        Self {
            decoder_cfgs: vec![attribute_decoder::Config::default()],
        }
    }
}

pub fn decode_attributes<W>(
    reader: &mut W,
    _cfg: Config,
    header: Header,
    mut decoded_attributes: Vec<Attribute>,
) -> Result<Vec<Attribute>, Err>
    where W: ByteReader,
{
    // Read the number of attributes
    let num_att_decs = reader.read_u8().unwrap() as usize;

    let mut att_dec_data_id = Vec::new();
    let mut att_dec_type = Vec::new();
    let mut att_dec_traversal_method = Vec::new();
    if header.encoding_method == EncoderMethod::Edgebreaker {
        for i in 0..num_att_decs  {
            att_dec_data_id.push(reader.read_u8()?);
            att_dec_type.push(AttributeKind::read_from(reader)?);
            att_dec_traversal_method.push(reader.read_u8()?);
        }
    }

    let mut att_dec_num_attributes = Vec::with_capacity(num_att_decs);
    let mut att_dec_att_types = Vec::with_capacity(num_att_decs);
    let mut att_dec_data_types = Vec::with_capacity(num_att_decs);
    let mut att_dec_num_components = Vec::with_capacity(num_att_decs);
    let mut att_dec_normalized = Vec::with_capacity(num_att_decs);
    let mut att_dec_unique_ids = Vec::with_capacity(num_att_decs);
    let mut seq_att_dec_decoder_type = Vec::with_capacity(num_att_decs);
    for i in 0.. num_att_decs {
        att_dec_num_attributes.push( leb128_read(reader)? as usize);
        att_dec_att_types.push(Vec::with_capacity(att_dec_num_attributes[i]));
        att_dec_data_types.push(Vec::with_capacity(att_dec_num_attributes[i]));
        att_dec_num_components.push(Vec::with_capacity(att_dec_num_attributes[i]));
        att_dec_normalized.push(Vec::with_capacity(att_dec_num_attributes[i]));
        att_dec_unique_ids.push(Vec::with_capacity(att_dec_num_attributes[i]));
        for j in 0..att_dec_num_attributes[i] {
            att_dec_att_types[i][j] = AttributeKind::read_from(reader)?;
            att_dec_data_types[i][j] = reader.read_u8()?;
            att_dec_num_components[i][j] = reader.read_u8()?;
            att_dec_normalized[i][j] = reader.read_u8()?;
            att_dec_unique_ids[i][j] = leb128_read(reader)?;
        }
        seq_att_dec_decoder_type.push(Vec::with_capacity(att_dec_num_attributes[i]));
        for j in 0..att_dec_num_attributes[i] {
            seq_att_dec_decoder_type[i][j] = reader.read_u8()?;
        }
    }

    let mut vertex_visited_point_ids = vec![0; num_att_decs as usize];
    let mut curr_att_dec = 0;

    // if header.encoding_method == EncoderMethod::Edgebreaker {
    //     decode_attribute_seams(reader)?;
    //     for i in 0..num_encoded_vertices + num_encoded_split_symbols {
    //         if is_vert_hole_[i] {
    //             update_vertex_to_corner_map(i);
    //         }
    //     }
    //     for i in 1..num_att_decs {
    //         curr_att_dec = i;
    //         recompute_vertices_internal();
    //     }
    //     attribute_assign_points_to_corners();
    // }
    // for i in 0..num_att_decs {
    //     curr_att_dec = i;
    //     is_face_visited_.assign(num_faces, false);
    //     is_vertex_visited_.assign(num_faces * 3, false);
    //     generate_sequence();
    //     if header.encoding_method == EncoderMethod::Edgebreaker {
    //         update_point_to_attribute_index_mapping();
    //     }
    // }
    // for i in 0..num_att_decs {
    //     for j in 0..att_dec_num_attributes[i] {
    //         att_dec_num_values_to_decode[i][j] = encoded_attribute_value_index_to_corner_map[i].size();
    //     }
    // }
    // for i in 0..num_att_decs {
    //     curr_att_dec = i;
    //     decode_portable_attributes();
    //     decode_ata_needed_by_portable_transforms();
    //     transform_attributes_to_original_format();
    // }

    Ok(decoded_attributes)
}