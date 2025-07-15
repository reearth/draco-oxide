use draco_oxide::io::gltf::transcoder::DracoTranscoder;
fn main() {
    // input file
    let input = "input.gltf";

    // output file
    let output = "output.glb";
    
    // Create transcoder with default options
    let mut transcoder = DracoTranscoder::create(None).unwrap();

    // Set up file options
    let file_options = draco_oxide::io::gltf::transcoder::FileOptions::new(
        input.to_string(),
        output.to_string(),
    );

    // transcode the GLTF file
    transcoder.transcode(&file_options).unwrap();
}