use draco::prelude::*;

#[test]
fn simplest() {
    let points = vec![
        NdVector::from([0.0, 0.0, 0.0]),
        NdVector::from([1.0, 1.0, 1.0]),
        NdVector::from([2.0, 2.0, 2.0]),
    ];

    let faces = vec![
        [0, 1, 2],
    ];
    
    let mesh = {
        let mut builder = MeshBuilder::new();
        let ref_face_att = builder.add_connectivity_attribute(faces, Vec::new());
        builder.add_attribute(points, AttributeType::Position, vec![ref_face_att]);
        builder.build()
    };

    let mesh = mesh.unwrap();
    let mut buff_writer = buffer::writer::Writer::new();
    let mut writer = |input| buff_writer.next(input);
    encode(mesh, &mut writer, Config::default()).unwrap();
    let data: Buffer = buff_writer.into();

    assert!(data.len() > 200, "Data length is less than 200");
}