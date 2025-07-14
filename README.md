# draco-oxcide

&#x20;&#x20;

`draco-oxide` is a high-performance Rust re-write of Google’s [Draco](https://github.com/google/draco) 3D-mesh compression library, featuring efficient streaming I/O and seamless WebAssembly integration.

> **Status:** **Alpha** – Encoder is functional; decoder implementation is **work‑in‑progress**.

---

## Features

| Component              | Alpha  | Beta Roadmap       |
| ------------------     | -----  | ------------------ |
| Mesh Encoder           | ✅     | Performance optimization |
| Mesh Decoder           | ❌     | ✅                  |
| GLTF Transcoder (basic)| ✅     | Animation and many more extensions  |

### Encoder Highlights

* Triangle‑mesh compression with configurable speed/ratio presets.
* Basic GLTF transcoder (`*.gltf` or `*.glb` → `*.glb` with mesh buffer compressed via [KHR_draco_mesh_compression extension](https://github.com/KhronosGroup/glTF/tree/main/extensions/2.0/Khronos/KHR_draco_mesh_compression)).
* Pure‑Rust implementation.
* `no_std` + `alloc` compatible; builds to **WASM32**, **x86\_64**, **aarch64**, and more.

### Decoder (Coming Soon)

Planned for the **beta** milestone.

---

## Getting Started

### Add to Your Project

```bash
cargo add draco-oxcide --git https://github.com/your-org/draco-oxcide --tag v0.1.0-alpha.*
```

> Until the first crates.io release, install directly from Git.

### Example: Encode a GLTF Scene

```rust
use draco_oxide::{encode::{self, encode}, io::obj::load_obj};
use draco_oxide::prelude::ConfigType;
use std::io::Write;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create mesh from an obj file, for example.
    let mesh = load_obj("mesh.obj").unwrap();

    // Create a buffer that we write the encoded data to.
    // This time we use 'Vec<u8>' as the output buffer, but draco-oxide can stream-write to anything 
    // that implements 'draco_oxide::prelude::ByteWriter'.
    let mut buffer = Vec::new();
    
    // Encode the mesh into the buffer.
    encode(mesh, &mut buffer, encode::Config::default()).unwrap();

    let mut file = std::fs::File::create("output.drc").unwrap();
    file.write_all(&buffer)?;
    Ok(())
}
```

See the [draco-oxide/examples](draco-oxide/examples/) directory for more.

### CLI

```bash
# compress input.obj into a draco file output.drc
cargo run --bin cli -- -i path/to/input.obj -o path/to/output.drc

# transcodes gltf.obj into a draco compressed glb file output.dlb as specified 
# in KHR_draco_mesh_compression extension.
cargo run --bin cli -- --transcode path/to/input.glb -o path/to/output.glb
```
---

## Roadmap to Beta

* Decoder Support.
* Complete glTF support.

---

## Acknowledgements

* **Google Draco** – original C++ implementation

---

## Contact

Re:Earth core committers: [community@reearth.io](mailto:community@reearth.io)

---

## License

Licensed under either (at your discretion):

- Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license
   ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
