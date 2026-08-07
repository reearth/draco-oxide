use draco_oxide::core::attribute::{Attribute, AttributeDomain, AttributeId, AttributeType};
use draco_oxide::core::types::{ConfigType, NdVector};
use draco_oxide::encode::{encode_point_cloud, PointCloudConfig};
use draco_oxide::{decode, PointCloud};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let positions: Vec<NdVector<3, f32>> = vec![
        NdVector::from([0.0, 0.0, 0.0]),
        NdVector::from([1.0, 2.0, 3.0]),
    ];
    let att = Attribute::from_without_removing_duplicates::<NdVector<3, f32>, 3>(
        AttributeId::new(0),
        positions,
        AttributeType::Position,
        AttributeDomain::Position,
        Vec::new(),
    );
    let point_cloud = PointCloud::new(vec![att])?;

    let mut buffer = Vec::new();
    encode_point_cloud(point_cloud, &mut buffer, PointCloudConfig::default())?;

    let decoded = decode::decode_point_cloud(&buffer)?;
    println!("{} points", decoded.num_points());
    Ok(())
}
