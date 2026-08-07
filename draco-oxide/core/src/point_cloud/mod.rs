//! The point cloud data model: one attribute value per point, no connectivity.

use crate::attribute::{Attribute, ComponentDataType};
use crate::mesh::Mesh;
use crate::types::{NdVector, PointIdx, Vector};

/// Errors produced while assembling a [`PointCloud`].
#[remain::sorted]
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Err {
    /// An attribute maps several points onto shared values.
    #[error("attribute values are deduplicated; a point cloud needs one value per point")]
    DeduplicatedAttribute,
    /// The attributes disagree on the number of points.
    #[error("attributes disagree on the number of points: {0} vs {1}")]
    MismatchedAttributeLengths(usize, usize),
    /// A point cloud needs at least one attribute.
    #[error("a point cloud needs at least one attribute")]
    NoAttributes,
    /// An attribute layout cannot be carried by a point cloud.
    #[error("unsupported attribute layout: {0} components of {1:?}")]
    UnsupportedAttributeLayout(usize, ComponentDataType),
}

/// Attributes over a shared point space; the i'th value of every attribute
/// belongs to the i'th point.
#[derive(Debug, Clone)]
pub struct PointCloud {
    attributes: Vec<Attribute>,
    num_points: usize,
}

impl PointCloud {
    /// Assembles a point cloud from attributes holding one value per point.
    pub fn new(attributes: Vec<Attribute>) -> Result<Self, Err> {
        let Some(first) = attributes.first() else {
            return Err(Err::NoAttributes);
        };
        let num_points = first.num_unique_values();
        for att in &attributes {
            if att.point_map_as_slice().is_some() {
                return Err(Err::DeduplicatedAttribute);
            }
            if att.num_unique_values() != num_points {
                return Err(Err::MismatchedAttributeLengths(
                    num_points,
                    att.num_unique_values(),
                ));
            }
        }
        Ok(Self {
            attributes,
            num_points,
        })
    }

    /// The number of points.
    pub fn num_points(&self) -> usize {
        self.num_points
    }

    /// The attributes, each holding one value per point.
    pub fn attributes(&self) -> &[Attribute] {
        &self.attributes
    }

    /// Mutable access to the attributes.
    pub fn attributes_mut(&mut self) -> &mut [Attribute] {
        &mut self.attributes
    }

    /// Consumes the point cloud, returning its attributes.
    pub fn into_attributes(self) -> Vec<Attribute> {
        self.attributes
    }
}

impl Mesh {
    /// Drops the faces, materializing one attribute value per point.
    pub fn into_point_cloud(self) -> Result<PointCloud, Err> {
        let attributes = self
            .attributes
            .into_iter()
            .map(materialize_per_point)
            .collect::<Result<Vec<_>, Err>>()?;
        PointCloud::new(attributes)
    }
}

/// Re-expands a deduplicated attribute onto the identity point-to-value map.
fn materialize_per_point(att: Attribute) -> Result<Attribute, Err> {
    if att.point_map_as_slice().is_none() {
        return Ok(att);
    }
    let num_components = att.get_num_components();
    let component_type = att.get_component_type();

    macro_rules! dispatch {
        ($(($ty:ty, $ct:ident)),* $(,)?) => {
            match (component_type, num_components) {
                $(
                    (ComponentDataType::$ct, 1) => materialize_typed::<NdVector<1, $ty>, 1>(att),
                    (ComponentDataType::$ct, 2) => materialize_typed::<NdVector<2, $ty>, 2>(att),
                    (ComponentDataType::$ct, 3) => materialize_typed::<NdVector<3, $ty>, 3>(att),
                    (ComponentDataType::$ct, 4) => materialize_typed::<NdVector<4, $ty>, 4>(att),
                )*
                _ => Err(Err::UnsupportedAttributeLayout(num_components, component_type)),
            }
        };
    }
    dispatch!(
        (i8, I8),
        (u8, U8),
        (i16, I16),
        (u16, U16),
        (i32, I32),
        (u32, U32),
        (f32, F32),
        (f64, F64),
    )
}

fn materialize_typed<Data, const N: usize>(att: Attribute) -> Result<Attribute, Err>
where
    Data: Vector<N>,
{
    let values: Vec<Data> = (0..att.len()).map(|p| att.get(PointIdx::from(p))).collect();
    let mut out = Attribute::from_without_removing_duplicates::<Data, N>(
        att.get_id(),
        values,
        att.get_attribute_type(),
        att.get_domain(),
        att.get_parents().clone(),
    );
    if let Some(name) = att.get_name() {
        out.set_name(name.clone());
    }
    Ok(out)
}
