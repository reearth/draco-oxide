//! Attribute decoding for sequential streams: values are sequenced linearly
//! over the point space rather than over a traversal, and every attribute is
//! carried by a single encoder.
//!
//! Without a corner table there is no mesh connectivity to predict from, so an
//! encoder can only pick delta prediction (or none), the point-to-value map is
//! the identity, and each attribute holds one value per point.

use super::{
    parse_payload_dispatched, read_portabilization, DecodedAttributes, Descriptor, Parsed,
    ParsedPayload, NO_ATTRIBUTE_DATA,
};
use crate::connectivity::SequentialConnectivity;
use crate::entropy::unzigzag;
use crate::Err;
use draco_oxide_core::attribute::{
    Attribute, AttributeDomain, AttributeId, AttributeType, ComponentDataType,
};
use draco_oxide_core::bit_coder::Reader;
use draco_oxide_core::codec::attribute::prediction_scheme::PredictionSchemeType;
use draco_oxide_core::codec::connectivity::edgebreaker::TraversalType;
use draco_oxide_core::types::{NdVector, Vector};
use draco_oxide_core::utils::bit_coder::leb128_read;

/// Decodes the attribute section of a sequential stream, positioned right after
/// connectivity.
pub(super) fn decode_attributes(
    reader: &mut Reader<'_>,
    conn: &SequentialConnectivity,
) -> Result<DecodedAttributes, Err> {
    if reader.read_u8()? != 1 {
        return Err(Err::MalformedAttribute(
            "a sequential stream carries all attributes in one encoder",
        ));
    }
    let descriptors = read_descriptors(reader)?;
    let num_points = conn.num_points;

    // One encoder carries every attribute, so it emits all the payload blocks
    // before the first portabilization block.
    let mut payloads = Vec::with_capacity(descriptors.len());
    for desc in &descriptors {
        payloads.push(parse_payload_dispatched(reader, num_points, desc)?);
    }
    let mut attributes = Vec::with_capacity(descriptors.len());
    let mut transforms = Vec::with_capacity(descriptors.len());
    for (desc, payload) in descriptors.iter().zip(payloads) {
        transforms.push(read_portabilization(reader, desc)?);
        attributes.push(decode_values(payload, desc, num_points)?);
    }

    Ok(DecodedAttributes {
        faces: conn.faces.clone(),
        attributes,
        transforms,
    })
}

/// Reads the descriptors of the stream's single attribute encoder: the shared
/// per-attribute fields for every attribute, then one portabilization type per
/// attribute.
fn read_descriptors(reader: &mut Reader<'_>) -> Result<Vec<Descriptor>, Err> {
    let num_atts = leb128_read(reader)? as usize;
    // Five bytes of descriptor per attribute is the floor, so anything past a
    // fifth of what is left cannot be honoured.
    if num_atts > reader.remaining() / 5 {
        return Err(Err::MalformedAttribute(
            "attribute count exceeds the stream",
        ));
    }
    let mut fields = Vec::with_capacity(num_atts);
    for _ in 0..num_atts {
        let att_type = AttributeType::read_from(reader)?;
        let component_type = ComponentDataType::read_from(reader)?;
        let num_components = reader.read_u8()? as usize;
        let _normalized = reader.read_u8()?;
        let uid = leb128_read(reader)? as u32;
        fields.push((att_type, component_type, num_components, uid));
    }

    let mut descriptors = Vec::with_capacity(num_atts);
    for (att_type, component_type, num_components, uid) in fields {
        descriptors.push(Descriptor {
            // Sequential attributes are per-point by construction, so neither
            // an attribute connectivity nor a corner domain can arise.
            att_data_id: NO_ATTRIBUTE_DATA,
            att_type,
            component_type,
            num_components,
            uid,
            port_type: reader.read_u8()?,
            domain: AttributeDomain::Position,
            // A sequential stream carries no connectivity to traverse.
            traversal: TraversalType::DepthFirst,
        });
    }
    Ok(descriptors)
}

/// Reverses prediction and the prediction transform over the linear point
/// sequence, behind the portable component-count dispatch.
fn decode_values(
    payload: ParsedPayload<'_>,
    desc: &Descriptor,
    num_points: usize,
) -> Result<Attribute, Err> {
    match payload {
        ParsedPayload::N1(p) => decode_values_typed::<1>(p, desc, num_points),
        ParsedPayload::N2(p) => decode_values_typed::<2>(p, desc, num_points),
        ParsedPayload::N3(p) => decode_values_typed::<3>(p, desc, num_points),
        ParsedPayload::N4(p) => decode_values_typed::<4>(p, desc, num_points),
    }
}

/// Walks the point sequence in order, reconstructing each value from its
/// correction and the value before it. The point-to-value map is left as the
/// identity.
fn decode_values_typed<const N: usize>(
    parsed: Parsed<'_, N>,
    desc: &Descriptor,
    num_points: usize,
) -> Result<Attribute, Err>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    let delta = match parsed.scheme_ty {
        PredictionSchemeType::DeltaPrediction => true,
        PredictionSchemeType::NoPrediction => false,
        _ => {
            return Err(Err::MalformedAttribute(
                "a sequential stream has no connectivity to predict from",
            ))
        }
    };

    let Parsed {
        mut corrections,
        transform,
        ..
    } = parsed;
    let zigzagged = transform.corrections_are_zigzagged();

    let mut values = Vec::with_capacity(num_points);
    let mut previous = NdVector::<N, i32>::zero();
    for k in 0..num_points {
        // SAFETY: the correction stream was parsed with `num_points` values,
        // and `k` runs over exactly that range in order.
        let mut corr = unsafe { corrections.next_unchecked(k) };
        if zigzagged {
            for i in 0..N {
                *corr.get_mut(i) = unzigzag(*corr.get(i) as u32);
            }
        }
        let prediction = if delta {
            previous
        } else {
            NdVector::<N, i32>::zero()
        };
        previous = transform.compute_original(prediction, corr);
        values.push(previous);
    }

    Ok(Attribute::from_without_removing_duplicates::<
        NdVector<N, i32>,
        N,
    >(
        AttributeId::new(desc.uid as usize),
        values,
        desc.att_type,
        desc.domain,
        Vec::new(),
    ))
}
