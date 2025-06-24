use draco::{encode::{self, encode}, prelude::{AttributeType, NdVector, ConfigType}, MeshBuilder};
use std::io::Write;
use std::process::Command;

/// Test that encodes a glTF file using our Rust encoder and then decodes it back
/// using Google's C++ Draco decoder to verify compatibility
// #[test]
fn test_gltf_encode_decode_compatibility() {
    // Load a simple glTF test file
    let gltf_file = "tests/data/sphere.gltf";
    
    // Convert glTF to our internal mesh representation
    let mesh = gltf_to_mesh(gltf_file).expect("Failed to load glTF file");
    
    // Encode using our Rust implementation
    let mut encoded_buffer = Vec::new();
    encode(mesh.clone(), &mut encoded_buffer, <encode::Config as ConfigType>::default())
        .expect("Failed to encode mesh");
    
    // Write the encoded data to a temporary file
    let output_path = "tests/outputs/sphere_gltf_encoded.drc";
    let mut file = std::fs::File::create(output_path)
        .expect("Failed to create output file");
    file.write_all(&encoded_buffer)
        .expect("Failed to write encoded data");
    
    // Decode using Google's C++ Draco decoder (draco_decoder)
    let decoded_output = decode_with_cpp_draco(output_path)
        .expect("Failed to decode with C++ Draco");
    
    // Verify the decoded mesh has the expected properties
    verify_decoded_mesh(&decoded_output, &mesh);
    
    println!("✓ glTF encode/decode compatibility test passed");
}

// #[test]
fn test_gltf_encode_multiple_files() {
    let test_files = [
        "tests/data/sphere.gltf",
        "tests/data/one_face_123.gltf",
        "tests/data/sphere_no_tangents.gltf",
    ];
    
    for gltf_file in &test_files {
        println!("Testing {}", gltf_file);
        
        let mesh = gltf_to_mesh(gltf_file)
            .expect(&format!("Failed to load {}", gltf_file));
        
        let mut encoded_buffer = Vec::new();
        encode(mesh.clone(), &mut encoded_buffer, encode::Config::default())
            .expect(&format!("Failed to encode {}", gltf_file));
        
        // Verify the encoded data is not empty and has reasonable size
        assert!(!encoded_buffer.is_empty(), "Encoded data should not be empty");
        assert!(encoded_buffer.len() > 20, "Encoded data should have reasonable minimum size");
        
        let filename = std::path::Path::new(gltf_file)
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap();
        let output_path = format!("tests/outputs/{}_gltf_encoded.drc", filename);
        
        let mut file = std::fs::File::create(&output_path)
            .expect("Failed to create output file");
        file.write_all(&encoded_buffer)
            .expect("Failed to write encoded data");
        
        println!("✓ Encoded {} to {}", gltf_file, output_path);
    }
}

/// Convert a glTF file to our internal Mesh representation
/// This calls into the draco::io::gltf module
fn gltf_to_mesh(gltf_path: &str) -> Result<draco::Mesh, Box<dyn std::error::Error>> {
    // Use the glTF loading function from our io module
    draco::io::gltf::load_gltf(gltf_path)
}

/// Decode a .drc file using Google's C++ Draco decoder
fn decode_with_cpp_draco(drc_path: &str) -> Result<String, Box<dyn std::error::Error>> {
    // Check if draco_decoder is available
    let output = Command::new("draco_decoder")
        .arg("--help")
        .output();
    
    if output.is_err() {
        return Err("draco_decoder not found. Please install Google's Draco C++ tools.".into());
    }
    
    // Decode the .drc file to a temporary PLY file
    let ply_output = format!("{}.ply", drc_path);
    
    let output = Command::new("draco_decoder")
        .arg("-i")
        .arg(drc_path)
        .arg("-o")
        .arg(&ply_output)
        .output()?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("draco_decoder failed: {}", stderr).into());
    }
    
    // Read the decoded PLY file
    let decoded_content = std::fs::read_to_string(&ply_output)?;
    
    // Clean up temporary file
    std::fs::remove_file(&ply_output).ok();
    
    Ok(decoded_content)
}

/// Verify that the decoded mesh matches expectations
fn verify_decoded_mesh(decoded_ply: &str, original_mesh: &draco::Mesh) {
    // Basic verification that the PLY file contains expected content
    assert!(decoded_ply.contains("ply"), "Decoded output should be a valid PLY file");
    assert!(decoded_ply.contains("vertex"), "Decoded mesh should contain vertices");
    assert!(decoded_ply.contains("face"), "Decoded mesh should contain faces");
    
    // TODO: Add more sophisticated verification:
    // - Compare vertex count
    // - Compare face count  
    // - Verify attribute types match
    // - Check geometric fidelity within tolerance
    
    println!("Decoded PLY contains {} lines", decoded_ply.lines().count());
}
