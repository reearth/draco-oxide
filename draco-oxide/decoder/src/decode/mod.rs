use draco_oxide_core::attribute::{AttributeType, ComponentDataType};
use draco_oxide_core::types::{CornerIdx, PointIdx};
use crate::prelude::{ByteReader, ConfigType, Mesh};

mod attribute;
mod connectivity;
mod entropy;
mod header;
mod metadata;

/// Decodes a Draco bitstream into a `Mesh`. Discards any non-fatal
/// warnings the decoder produced — use [`decode_with_warnings`] when
/// you need to detect attributes that were silently skipped because
/// the decoder doesn't yet support their prediction scheme / transform.
pub fn decode<R>(reader: &mut R, cfg: Config) -> Result<Mesh, Err>
where
    R: ByteReader,
{
    decode_with_warnings(reader, cfg).map(|(mesh, _warnings)| mesh)
}

/// Like [`decode`] but also returns any [`DecodeWarning`]s the pipeline
/// surfaced. A non-empty warnings list means the returned `Mesh` is
/// partial — typically an attribute with an unsupported prediction
/// scheme was skipped, and downstream consumers should branch
/// accordingly (fall back to a flat shading, ask for a re-render, etc.)
/// rather than treat the mesh as fully decoded.
///
/// Pipeline (mirrors `encode/mod.rs::encode`):
/// 1. header → `decode/header`
/// 2. metadata (if header flags say so) → `decode/metadata`
/// 3. connectivity → `decode/connectivity`
/// 4. attributes → `decode/attribute`
pub fn decode_with_warnings<R>(
    reader: &mut R,
    _cfg: Config,
) -> Result<(Mesh, Vec<DecodeWarning>), Err>
where
    R: ByteReader,
{
    let header = header::decode_header(reader).map_err(Err::Header)?;

    if header.contains_metadata {
        let _metadata = metadata::decode_metadata(reader).map_err(Err::Metadata)?;
    }

    let conn = connectivity::decode_connectivity(reader, &header).map_err(Err::Connectivity)?;

    let mut warnings = Vec::new();
    let attrs = attribute::decode_attributes(
        reader,
        &header,
        &conn.corner_table,
        &conn.attribute_corner_tables,
        &conn.start_corners,
        conn.num_position_vertices,
        attribute::Config {},
        &mut warnings,
    )
    .map_err(Err::Attribute)?;

    // Compact face vertex IDs to a contiguous range matching the per-attribute
    // decode order. Each attribute is stored in vertex-id-ascending order
    // over the visited slots in `partial[]`. Faces reference the
    // (alias-resolved) corner-table vertex IDs, which can include "real"
    // IDs above num_position_vertices when add_new_vertex outpaces S-merges.
    // Build remap[old_vertex_id] = rank-among-unique-face-IDs and apply.
    let sorted_ids: Vec<usize> = {
        let mut s = std::collections::BTreeSet::new();
        for f in &conn.faces {
            for v in f {
                s.insert(usize::from(*v));
            }
        }
        s.into_iter().collect()
    };

    let max_id = sorted_ids.last().copied().unwrap_or(0);
    let mut remap = vec![usize::MAX; max_id + 1];
    for (rank, &id) in sorted_ids.iter().enumerate() {
        remap[id] = rank;
    }

    let mut mesh = Mesh::new();
    mesh.faces = conn
        .faces
        .into_iter()
        .map(|f| {
            [
                PointIdx::from(remap[usize::from(f[0])]),
                PointIdx::from(remap[usize::from(f[1])]),
                PointIdx::from(remap[usize::from(f[2])]),
            ]
        })
        .collect();
    mesh.attributes = attrs;
    Ok((mesh, warnings))
}

#[derive(Debug, Clone)]
pub struct Config {}

impl ConfigType for Config {
    fn default() -> Self {
        Self {}
    }
}

#[remain::sorted]
#[derive(thiserror::Error, Debug)]
pub enum Err {
    #[error("Attribute decoding error: {0}")]
    Attribute(#[from] attribute::Err),
    #[error("Connectivity decoding error: {0}")]
    Connectivity(#[from] connectivity::Err),
    #[error(
        "Mesh has {num_faces} faces; the flat-buffer index format caps \
         total corners at u32::MAX"
    )]
    FaceCountOverflow { num_faces: usize },
    #[error("Header decoding error: {0}")]
    Header(#[from] header::Err),
    #[error(
        "Corner {corner} references vertex {vertex} which has no decoded \
         position — the connectivity and attribute streams are inconsistent"
    )]
    InconsistentCornerVertex { corner: usize, vertex: usize },
    #[error("Metadata decoding error: {0}")]
    Metadata(#[from] metadata::Err),
}

/// Non-fatal issue surfaced during decode. Returned alongside the
/// `Mesh` / `DecodedRaw` from the `_with_warnings` API variants.
#[derive(Debug, Clone)]
pub enum DecodeWarning {
    /// An attribute couldn't be decoded because its prediction
    /// scheme, transform, or component layout isn't yet supported.
    /// Earlier attributes in the same primitive were decoded
    /// normally; the returned mesh just lacks this one. After this
    /// fires, the byte stream is in an undefined state — no further
    /// attributes are read.
    AttributeSkipped {
        /// Position of the skipped attribute in the bitstream's
        /// per-primitive attribute list (0 = first, etc.).
        attribute_index: usize,
        /// What kind of attribute was being decoded.
        attribute_type: AttributeType,
        /// Human-readable description of the missing decoder path.
        reason: String,
    },
}

/// Flat byte buffer holding the index list immediately followed by each
/// attribute's value array, plus per-attribute offsets/lengths so the
/// caller can splice the bytes straight into a glTF binary buffer.
///
/// Layout invariants:
/// - `data[0..indices_byte_length]` is the index list, encoded as
///   `indices_component_type` (`U16` if `vertex_count <= 65_535`,
///   else `U32`). It always has `index_count` entries.
/// - For each `RawAttribute a`, `data[a.offset..a.offset + a.byte_length]`
///   is `vertex_count` rows of `a.dim` components, each `a.component_type`
///   wide. Offsets are 4-byte aligned.
/// - All attributes share the same `vertex_count`. Per-attribute
///   corner-table seams (NORMAL/TEXCOORD splitting a position vertex)
///   are resolved by dedup-by-tuple over each face's three corners, so
///   indices reference value rows that are consistent across every
///   attribute — what every glTF loader expects.
/// - `attributes` are ordered to match the per-attribute decode order
///   (= the order Draco wrote them, = the order the encoder side
///   `attributes` map references via `unique_id`).
#[derive(Debug, Clone)]
pub struct DecodedRaw {
    pub data: Vec<u8>,
    pub vertex_count: u32,
    pub index_count: u32,
    pub indices_offset: usize,
    pub indices_byte_length: usize,
    pub indices_component_type: ComponentDataType,
    pub attributes: Vec<RawAttribute>,
}

/// Per-attribute slice description for [`DecodedRaw::data`].
#[derive(Debug, Clone)]
pub struct RawAttribute {
    /// Draco unique-id for this attribute. Mirrors the value in the
    /// glTF extension's `attributes` map (POSITION, NORMAL, TEXCOORD_0,
    /// …) so callers can route attributes back to their semantic name.
    pub unique_id: u32,
    /// Byte offset into [`DecodedRaw::data`].
    pub offset: usize,
    /// Total byte length of this attribute's value array. Equal to
    /// `vertex_count * dim * component_type.size()`.
    pub byte_length: usize,
    /// Number of components per vertex (3 for POSITION, 2 for TEXCOORD,
    /// 3 for NORMAL, 4 for COLOR with alpha, etc.).
    pub dim: u8,
    /// Component scalar type. Use [`ComponentDataType::to_gltf_component_type`]
    /// to convert to a glTF `componentType` code.
    pub component_type: ComponentDataType,
    /// Best-effort glTF semantic name (POSITION, NORMAL, TEXCOORD_0,
    /// COLOR_0, ...). `None` for custom/unknown attributes — fall back
    /// to `unique_id` lookups in that case.
    pub gltf_semantic: Option<&'static str>,
}

/// Like [`decode`] but produces a flat raw-byte buffer suitable for
/// splicing straight into a glTF binary buffer. See [`DecodedRaw`] for
/// the layout contract.
///
/// For meshes whose attributes use per-attribute corner tables (i.e.
/// NORMAL or TEXCOORD have seams that split a position vertex into
/// multiple per-attribute vertices — typical of photogrammetry 3D
/// Tiles), this function dedupes per-corner attribute tuples to produce
/// one unified `vertex_count` that all accessors share, with sequential
/// indices. This is what every standard glTF loader expects.
pub fn decode_to_raw<R>(reader: &mut R, cfg: Config) -> Result<DecodedRaw, Err>
where
    R: ByteReader,
{
    decode_to_raw_with_warnings(reader, cfg).map(|(raw, _warnings)| raw)
}

/// Like [`decode_to_raw`] but also returns any [`DecodeWarning`]s the
/// pipeline surfaced. Same partial-decode semantics as
/// [`decode_with_warnings`]: a non-empty warnings list means at least
/// one attribute couldn't be decoded and is missing from the output.
pub fn decode_to_raw_with_warnings<R>(
    reader: &mut R,
    _cfg: Config,
) -> Result<(DecodedRaw, Vec<DecodeWarning>), Err>
where
    R: ByteReader,
{
    let header = header::decode_header(reader).map_err(Err::Header)?;
    if header.contains_metadata {
        let _metadata = metadata::decode_metadata(reader).map_err(Err::Metadata)?;
    }

    // Sequential meshes have no corner table; they decode along a separate,
    // simpler path (raw triangle list + point-order attributes).
    if header.encoding_method == draco_oxide_core::codec::header::EncoderMethod::Sequential {
        let seq = connectivity::sequential::decode(reader, &header).map_err(|e| {
            Err::Connectivity(connectivity::Err::Sequential(e))
        })?;
        let attrs = attribute::decode_attributes_sequential(reader, &header, seq.num_points)
            .map_err(Err::Attribute)?;
        let raw = build_raw_sequential(&seq.faces, seq.num_points, &attrs)?;
        return Ok((raw, Vec::new()));
    }

    let conn = connectivity::decode_connectivity(reader, &header).map_err(Err::Connectivity)?;
    let mut warnings = Vec::new();
    let attrs = attribute::decode_attributes_with_meta(
        reader,
        &header,
        &conn.corner_table,
        &conn.attribute_corner_tables,
        &conn.start_corners,
        conn.num_position_vertices,
        attribute::Config {},
        &mut warnings,
    )
    .map_err(Err::Attribute)?;
    let raw = build_raw(&conn, &attrs)?;
    Ok((raw, warnings))
}

/// Minimal FxHash-style hasher (dependency-free). The vertex-dedup map below
/// hashes small tuples of `u32` attribute-value indices once per corner — std's
/// default SipHash is cryptographic and dominates `build_raw` on large meshes,
/// so a fast multiply-rotate hash cuts that phase ~4×. Collisions only cost
/// speed, not correctness.
#[derive(Default)]
struct FxHasher {
    hash: u64,
}

impl std::hash::Hasher for FxHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        const K: u64 = 0x51_7c_c1_b7_27_22_0a_95;
        for &b in bytes {
            self.hash = (self.hash.rotate_left(5) ^ b as u64).wrapping_mul(K);
        }
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        const K: u64 = 0x51_7c_c1_b7_27_22_0a_95;
        self.hash = (self.hash.rotate_left(5) ^ i as u64).wrapping_mul(K);
    }
}

type FxBuildHasher = std::hash::BuildHasherDefault<FxHasher>;

fn build_raw(
    conn: &connectivity::DecodedConnectivity,
    attrs: &[(draco_oxide_core::attribute::Attribute, Option<u8>)],
) -> Result<DecodedRaw, Err> {
    use draco_oxide_core::attribute::AttributeType;
    use draco_oxide_core::corner_table::GenericCornerTable;
    use std::collections::hash_map::Entry;
    use std::collections::HashMap;

    let num_faces = conn.faces.len();
    let num_corners = num_faces * 3;
    if num_corners > u32::MAX as usize {
        return Err(Err::FaceCountOverflow { num_faces });
    }

    // Build the universal-vertex-id → position-attribute-value-index map.
    // Position attribute values are output in vertex-id-ascending order
    // over visited universal vertices, and faces only reference visited
    // ones, so the sorted set of face vertex IDs gives the right ranking.
    let sorted_pos_ids: Vec<usize> = {
        let mut all: Vec<usize> = Vec::with_capacity(num_corners);
        for f in &conn.faces {
            for v in f {
                all.push(usize::from(*v));
            }
        }
        all.sort_unstable();
        all.dedup();
        all
    };
    let max_universal_id = sorted_pos_ids.last().copied().unwrap_or(0);
    let mut universal_to_pos_value: Vec<u32> = vec![u32::MAX; max_universal_id + 1];
    for (rank, &id) in sorted_pos_ids.iter().enumerate() {
        universal_to_pos_value[id] = rank as u32;
    }

    let n_attrs = attrs.len();
    let index_count = num_corners as u32;

    // An attribute is "seamed" only if it owns a per-attribute corner table
    // (decoder_id in range). When NONE are seamed, every attribute is indexed
    // by the universal position vertex, so a Draco "point" is exactly a
    // position vertex: there's no per-corner combination to deduplicate, and
    // each attribute's decoded values are already in output-vertex order. This
    // is the common case (position-only, 3D-Tiles pos/normal/uv) and lets us
    // skip the whole hash-dedup + gather that Google likewise avoids by
    // carrying its point structure through.
    let all_universal = attrs.iter().all(|(_, decoder_id)| {
        decoder_id
            .filter(|idx| (*idx as usize) < conn.attribute_corner_tables.len())
            .is_none()
    });

    // `indices`: corner -> output vertex. `out_src` (Some) is a flat
    // `output_vertex * n_attrs` table of source attribute-value indices for the
    // serializer to gather from; `None` means identity (source == output
    // vertex, i.e. a straight memcpy of each attribute's buffer).
    let (indices, vertex_count, out_src): (Vec<u32>, u32, Option<Vec<u32>>) = if all_universal {
        let mut indices = Vec::with_capacity(num_corners);
        for c in 0..num_corners {
            let universal_v = usize::from(conn.corner_table.vertex_idx(CornerIdx::from(c)));
            let rank = universal_to_pos_value
                .get(universal_v)
                .copied()
                .filter(|&r| r != u32::MAX)
                .ok_or(Err::InconsistentCornerVertex {
                    corner: c,
                    vertex: universal_v,
                })?;
            indices.push(rank);
        }
        (indices, sorted_pos_ids.len() as u32, None)
    } else {
        // Per-corner attribute-value-index table, flat with an `n_attrs`-wide
        // stride so each corner's tuple is contiguous (cache-friendly).
        let mut per_corner_indices: Vec<u32> = vec![0; num_corners * n_attrs];
        for (a, (_, decoder_id)) in attrs.iter().enumerate() {
            let attr_table_idx =
                decoder_id.filter(|idx| (*idx as usize) < conn.attribute_corner_tables.len());
            match attr_table_idx {
                None => {
                    for c in 0..num_corners {
                        let universal_v =
                            usize::from(conn.corner_table.vertex_idx(CornerIdx::from(c)));
                        let value_idx = universal_to_pos_value
                            .get(universal_v)
                            .copied()
                            .filter(|&r| r != u32::MAX)
                            .ok_or(Err::InconsistentCornerVertex {
                                corner: c,
                                vertex: universal_v,
                            })?;
                        per_corner_indices[c * n_attrs + a] = value_idx;
                    }
                }
                Some(idx) => {
                    let attr_table = &conn.attribute_corner_tables[idx as usize];
                    for c in 0..num_corners {
                        per_corner_indices[c * n_attrs + a] = attr_table.corner_to_vertex[c] as u32;
                    }
                }
            }
        }

        // Dedup per-corner tuples -> output vertices. For n_attrs <= 4 the
        // tuple fits a fixed `[u32; 4]` key (unused slots zero) — a single
        // allocation-free, wasm-friendly key (NB: u128 keys would be
        // software-emulated on 32-bit wasm and tank the browser path). Wider
        // tuples fall back to a Vec key.
        let mut indices = Vec::with_capacity(num_corners);
        let mut out_src: Vec<u32> = Vec::new();
        if n_attrs <= 4 {
            let mut map: HashMap<[u32; 4], u32, FxBuildHasher> =
                HashMap::with_capacity_and_hasher(num_corners, FxBuildHasher::default());
            for c in 0..num_corners {
                let slice = &per_corner_indices[c * n_attrs..(c + 1) * n_attrs];
                let mut key = [0u32; 4];
                key[..n_attrs].copy_from_slice(slice);
                let id = match map.entry(key) {
                    Entry::Occupied(e) => *e.get(),
                    Entry::Vacant(e) => {
                        let id = (out_src.len() / n_attrs) as u32;
                        out_src.extend_from_slice(slice);
                        e.insert(id);
                        id
                    }
                };
                indices.push(id);
            }
        } else {
            let mut map: HashMap<Vec<u32>, u32, FxBuildHasher> =
                HashMap::with_capacity_and_hasher(num_corners, FxBuildHasher::default());
            for c in 0..num_corners {
                let slice = &per_corner_indices[c * n_attrs..(c + 1) * n_attrs];
                let id = match map.get(slice) {
                    Some(&id) => id,
                    None => {
                        let id = (out_src.len() / n_attrs) as u32;
                        out_src.extend_from_slice(slice);
                        map.insert(slice.to_vec(), id);
                        id
                    }
                };
                indices.push(id);
            }
        }
        let vertex_count = (out_src.len() / n_attrs) as u32;
        (indices, vertex_count, Some(out_src))
    };

    let (idx_ct, idx_size) = if vertex_count <= u16::MAX as u32 {
        (ComponentDataType::U16, 2usize)
    } else {
        (ComponentDataType::U32, 4usize)
    };
    let indices_byte_length = (index_count as usize) * idx_size;

    let mut data: Vec<u8> = Vec::with_capacity(indices_byte_length + vertex_count as usize * 32);
    if idx_size == 2 {
        for &idx in &indices {
            data.extend_from_slice(&(idx as u16).to_le_bytes());
        }
    } else {
        for &idx in &indices {
            data.extend_from_slice(&idx.to_le_bytes());
        }
    }
    align_to_4(&mut data);

    let mut raw_attributes: Vec<RawAttribute> = Vec::with_capacity(n_attrs);
    let mut tex_coord_n: u32 = 0;
    let mut color_n: u32 = 0;

    for (a_idx, (att, _)) in attrs.iter().enumerate() {
        let dim = att.get_num_components() as u8;
        let component_type = att.get_component_type();
        let elem_size = component_type.size() * dim as usize;
        let src_bytes = att.get_data_as_bytes();

        let offset = data.len();
        let want = vertex_count as usize * elem_size;
        data.reserve(want);
        match &out_src {
            // Unseamed: the attribute's values are already in output-vertex
            // order — straight copy (with a zero-fill guard for the rare case
            // of fewer decoded values than vertices).
            None => {
                if want <= src_bytes.len() {
                    data.extend_from_slice(&src_bytes[..want]);
                } else {
                    data.extend_from_slice(src_bytes);
                    data.resize(offset + want, 0);
                }
            }
            // Seamed: gather each output vertex's source value.
            Some(out_src) => {
                for v in 0..vertex_count as usize {
                    let value_idx = out_src[v * n_attrs + a_idx] as usize;
                    let start = value_idx * elem_size;
                    let end = start + elem_size;
                    // Guard: if the decoder produced fewer values than the
                    // attribute corner table promised (degenerate cases),
                    // zero-fill the missing rows rather than panicking.
                    if end <= src_bytes.len() {
                        data.extend_from_slice(&src_bytes[start..end]);
                    } else {
                        data.resize(data.len() + elem_size, 0);
                    }
                }
            }
        }
        let byte_length = want;
        align_to_4(&mut data);

        let gltf_semantic: Option<&'static str> = match att.get_attribute_type() {
            AttributeType::Position => Some("POSITION"),
            AttributeType::Normal => Some("NORMAL"),
            AttributeType::Tangent => Some("TANGENT"),
            AttributeType::TextureCoordinate => {
                let s = match tex_coord_n {
                    0 => "TEXCOORD_0",
                    1 => "TEXCOORD_1",
                    _ => "TEXCOORD_N",
                };
                tex_coord_n += 1;
                Some(s)
            }
            AttributeType::Color => {
                let s = match color_n {
                    0 => "COLOR_0",
                    1 => "COLOR_1",
                    _ => "COLOR_N",
                };
                color_n += 1;
                Some(s)
            }
            _ => None,
        };

        raw_attributes.push(RawAttribute {
            unique_id: att.get_id().as_usize() as u32,
            offset,
            byte_length,
            dim,
            component_type,
            gltf_semantic,
        });
    }

    Ok(DecodedRaw {
        data,
        vertex_count,
        index_count,
        indices_offset: 0,
        indices_byte_length,
        indices_component_type: idx_ct,
        attributes: raw_attributes,
    })
}

/// Builds [`DecodedRaw`] for a SEQUENTIAL mesh. Far simpler than [`build_raw`]:
/// the triangle list indexes points directly and each attribute already holds
/// one value per point in order, so there's no corner-table dedup — indices
/// are the faces as-is and `vertex_count == num_points`.
fn build_raw_sequential(
    faces: &[[u32; 3]],
    num_points: usize,
    attrs: &[(draco_oxide_core::attribute::Attribute, Option<u8>)],
) -> Result<DecodedRaw, Err> {
    use draco_oxide_core::attribute::AttributeType;

    let num_faces = faces.len();
    let index_count = (num_faces * 3) as u32;
    let vertex_count = num_points as u32;
    if num_faces * 3 > u32::MAX as usize {
        return Err(Err::FaceCountOverflow { num_faces });
    }

    let (idx_ct, idx_size) = if vertex_count <= u16::MAX as u32 {
        (ComponentDataType::U16, 2usize)
    } else {
        (ComponentDataType::U32, 4usize)
    };
    let indices_byte_length = index_count as usize * idx_size;

    let mut data: Vec<u8> = Vec::with_capacity(indices_byte_length + num_points * 32);
    for f in faces {
        for &v in f {
            if idx_size == 2 {
                data.extend_from_slice(&(v as u16).to_le_bytes());
            } else {
                data.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
    align_to_4(&mut data);

    let mut raw_attributes: Vec<RawAttribute> = Vec::with_capacity(attrs.len());
    let mut tex_coord_n: u32 = 0;
    let mut color_n: u32 = 0;
    for (att, _) in attrs {
        let dim = att.get_num_components() as u8;
        let component_type = att.get_component_type();
        let elem_size = component_type.size() * dim as usize;
        let src_bytes = att.get_data_as_bytes();
        let want = num_points * elem_size;
        let offset = data.len();
        if src_bytes.len() >= want {
            data.extend_from_slice(&src_bytes[..want]);
        } else {
            data.extend_from_slice(src_bytes);
            data.resize(offset + want, 0);
        }
        align_to_4(&mut data);

        let gltf_semantic: Option<&'static str> = match att.get_attribute_type() {
            AttributeType::Position => Some("POSITION"),
            AttributeType::Normal => Some("NORMAL"),
            AttributeType::Tangent => Some("TANGENT"),
            AttributeType::TextureCoordinate => {
                let s = match tex_coord_n {
                    0 => "TEXCOORD_0",
                    1 => "TEXCOORD_1",
                    _ => "TEXCOORD_N",
                };
                tex_coord_n += 1;
                Some(s)
            }
            AttributeType::Color => {
                let s = match color_n {
                    0 => "COLOR_0",
                    1 => "COLOR_1",
                    _ => "COLOR_N",
                };
                color_n += 1;
                Some(s)
            }
            _ => None,
        };

        raw_attributes.push(RawAttribute {
            unique_id: att.get_id().as_usize() as u32,
            offset,
            byte_length: want,
            dim,
            component_type,
            gltf_semantic,
        });
    }

    Ok(DecodedRaw {
        data,
        vertex_count,
        index_count,
        indices_offset: 0,
        indices_byte_length,
        indices_component_type: idx_ct,
        attributes: raw_attributes,
    })
}

#[inline]
fn align_to_4(buf: &mut Vec<u8>) {
    let pad = (4 - (buf.len() % 4)) % 4;
    for _ in 0..pad {
        buf.push(0);
    }
}
