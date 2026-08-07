use std::{cmp, fmt};

use crate::encode::entropy::rans::{self, RabsCoder};
use crate::encode::entropy::symbol_coding::encode_symbols;
use draco_oxide_core::attribute::AttributeType;
use draco_oxide_core::bit_coder::{BitWriter, ByteWriter};
use draco_oxide_core::buffer::LsbFirst;
use draco_oxide_core::codec::connectivity::edgebreaker::symbol_encoder::{
    CrLight, Symbol, SymbolEncoder,
};
use draco_oxide_core::debug_write;
use draco_oxide_core::mesh::ds::CornerTable;
use draco_oxide_core::mesh::ds::GenericCornerTable;
use draco_oxide_core::mesh::ds::{AttributeDS, DS};

use draco_oxide_core::types::{
    ConfigType, CornerIdx, FaceIdx, VecCornerIdx, VecFaceIdx, VecVertexIdx, VertexIdx,
};

use draco_oxide_core::codec::connectivity::edgebreaker::{
    self, EdgebreakerKind, Orientation, TopologySplit, MAX_VALENCE, MIN_VALENCE,
};
use draco_oxide_core::codec::entropy::SymbolEncodingMethod;
use draco_oxide_core::utils::bit_coder::leb128_write;
use std::vec;

use crate::encode::connectivity::ConnectivityEncoder;

pub(crate) struct Edgebreaker<'ads, 'faces, T>
where
    T: Traversal,
{
    /// The 'i'th entry of 'visited_vertices' is true if the Edgebreaker has
    /// already visited the 'i' th vertex.
    visited_vertices: VecVertexIdx<bool>,

    /// The 'i'th entry of 'visited_edges' is true if the Edgebreaker has
    /// already visited the 'i' th face.
    visited_faces: VecFaceIdx<bool>,

    /// The visited holes. i th entry of this array records whether the i th hole is visited or not.
    visited_holes: Vec<bool>,

    // A map from vertices to the hole id if the vertex is on a hole or void if the vertex is not on a hole.
    vertex_hole_id: VecVertexIdx<Option<usize>>,

    corner_traversal_stack: Vec<CornerIdx>,

    last_encoded_symbol_idx: usize,

    processed_connectivity_corners: Vec<CornerIdx>,

    /// Per-face index of the S symbol that split it, or `u32::MAX` if the face
    /// carries no split. Symbol indices fit `u32` because every symbol consumes
    /// a face and face indices are `u32`-backed.
    face_to_split_symbol_map: VecFaceIdx<u32>,

    num_split_symbols: usize,

    vertex_traversal_length: Vec<usize>,

    init_face_connectivity_corners: Vec<CornerIdx>,

    traversal: T,

    /// Records the topology splits detected during the edgebreaker encoding.
    topology_splits: Vec<TopologySplit>,

    adss: &'ads [AttributeDS<'faces>],

    pos_corner_table: &'ads CornerTable,

    posds: &'ads AttributeDS<'faces>,

    gds: &'ads DS,

    /// configurations for the encoder
    config: Config,
}

/// Configuration for edgebreaker connectivity encoding. Exported as
/// `EdgebreakerConfig`.
#[derive(Clone, fmt::Debug, cmp::PartialEq)]
pub struct Config {
    /// The edgebreaker variant used to traverse the mesh and code the
    /// topology symbols.
    pub traversal: EdgebreakerKind,
}

impl ConfigType for Config {
    fn default() -> Self {
        Self {
            traversal: EdgebreakerKind::Valence,
        }
    }
}

#[derive(Debug, PartialEq)]
#[remain::sorted]
#[derive(thiserror::Error)]
pub enum Err {
    #[error("Edgebreaker error: {0}")]
    EdgebreakerError(#[from] edgebreaker::Err),
    #[error("The input mesh has an empty AttributeDS array.")]
    EmptyAttributeDSArray,
    #[error("Entropy encoding error: {0}")]
    EntropyEncodingError(#[from] crate::encode::entropy::symbol_coding::Err),
    #[error("Too many handles.")]
    HandleSizeTooLarge,
    #[error("Too many holes.")]
    HoleSizeTooLarge,
    #[error("The input mesh is non-orientable.")]
    NonOrientable,
    #[error("Rabs coder error: {0}")]
    RabsCoderError(#[from] rans::Err),
    #[error("The input mesh has too many connected components: {0}")]
    TooManyConnectedComponents(usize),
}

impl<'ads, 'faces, T> Edgebreaker<'ads, 'faces, T>
where
    T: Traversal,
{
    // Build the object with empty arrays.
    pub fn new(
        config: Config,
        adss: &'ads [AttributeDS<'faces>],
        make_traversal: impl FnOnce(&'ads AttributeDS<'faces>) -> T,
    ) -> Result<Self, Err> {
        let pos_ads = adss
            .iter()
            .find(|ads| ads.att_data().get_attribute_type() == AttributeType::Position)
            .ok_or(Err::EmptyAttributeDSArray)?;
        let gds = pos_ads.global_ds();
        let traversal = make_traversal(pos_ads);

        let out = Self {
            visited_vertices: VecVertexIdx::from(vec![false; pos_ads.num_vertices()]),
            visited_faces: VecFaceIdx::from(vec![false; gds.num_faces()]),
            visited_holes: Vec::new(),
            pos_corner_table: pos_ads.corner_table().pos_corner_table(),
            posds: pos_ads,
            vertex_hole_id: VecVertexIdx::new(),
            corner_traversal_stack: Vec::new(),
            last_encoded_symbol_idx: usize::MAX,
            processed_connectivity_corners: Vec::new(),
            face_to_split_symbol_map: VecFaceIdx::from(vec![u32::MAX; gds.num_faces()]),
            num_split_symbols: 0,
            vertex_traversal_length: Vec::new(),
            init_face_connectivity_corners: Vec::new(),
            traversal,
            topology_splits: Vec::new(),
            gds,
            adss,
            config,
        };
        Ok(out)
    }

    fn compute_boundaries(&mut self) -> Result<(), Err> {
        self.vertex_hole_id = VecVertexIdx::from(vec![None; self.posds.num_vertices()]);
        for c in 0..self.gds.num_corners() {
            let c = CornerIdx::from(c);
            if self.pos_corner_table.opposite(c).is_none() {
                // 'c' is on a boundary.
                let mut v = self.posds.vertex_idx(c.next());
                if self.vertex_hole_id[v].is_some() {
                    // The hole is already processed.
                    continue;
                }
                // Now we have found a new boundary containing the vertex 'v'.
                let boundary_idx = self.visited_holes.len();
                self.visited_holes.push(false);

                let mut c = c;
                while self.vertex_hole_id[v].is_none() {
                    self.vertex_hole_id[v] = Some(boundary_idx);
                    c = c.next();

                    while self.pos_corner_table.opposite(c).is_some() {
                        c = c.next();
                    }
                    // Id of the next vertex in the vertex on the hole.
                    v = self.posds.vertex_idx(c.next());
                }
            }
        }
        Ok(())
    }

    fn process_boundary(&mut self, start_corner: CornerIdx, encode_first_vertex: bool) -> usize {
        let mut corner = start_corner.previous();
        while let Some(opp) = self.pos_corner_table.opposite(corner) {
            corner = opp.next();
        } // 'corner' now faces the hole

        let start_v = self.posds.vertex_idx(start_corner);

        let mut num_encoded_hole_verts = 0;
        if encode_first_vertex {
            self.visited_vertices[start_v] = true;
            num_encoded_hole_verts += 1;
        }

        self.visited_holes[self.vertex_hole_id[start_v].unwrap()] = true; // it is safe to unwrap here as start_v is on a hole.
        let mut curr_v = self.posds.vertex_idx(corner.previous());
        while curr_v != start_v {
            self.visited_vertices[curr_v] = true;
            num_encoded_hole_verts += 1;
            corner = corner.next();
            while let Some(opp) = self.pos_corner_table.opposite(corner) {
                corner = opp.next();
            }
            curr_v = self.posds.vertex_idx(corner.previous());
        }
        num_encoded_hole_verts
    }

    /// A function implementing the Edgebreaker algorithm for a connected component that contains `c`.
    fn edgebreaker_from(&mut self, mut c: CornerIdx) -> Result<(), Err> {
        self.corner_traversal_stack.clear();
        self.corner_traversal_stack.push(c);
        let num_faces = self.gds.num_faces();
        while let Some(&start) = self.corner_traversal_stack.last() {
            c = start;
            // Make sure the face hasn't been visited yet.
            if self.visited_faces[c.face_idx()] {
                self.corner_traversal_stack.pop();
                continue;
            }

            let mut num_visited_faces = 0;
            while num_visited_faces < num_faces {
                num_visited_faces += 1;
                self.last_encoded_symbol_idx = self.last_encoded_symbol_idx.wrapping_add(1); // since the initial value of 'last_encoded_symbol_idx' is usize::MAX, we do wrapping-add.

                let face_idx = c.face_idx();
                self.visited_faces[face_idx] = true;
                self.processed_connectivity_corners.push(c);
                self.traversal.new_corner_reached(c);
                let v = self.posds.vertex_idx(c);
                if !self.visited_vertices[v] {
                    self.visited_vertices[v] = true;
                    if self.vertex_hole_id[v].is_none() {
                        self.traversal.record_symbol(
                            Symbol::C,
                            &self.visited_faces,
                            self.pos_corner_table,
                        );
                        c = self.posds.corner_table().get_right_corner(c).unwrap(); // unwrap is safe here; we checked that the right edge is not on a boundary, and this implies that the right face exists.
                        continue;
                    }
                }
                let maybe_right_c = self.posds.corner_table().get_right_corner(c);
                let maybe_left_c = self.posds.corner_table().get_left_corner(c);
                let maybe_right_face = maybe_right_c.map(|c| c.face_idx());
                let maybe_left_face = maybe_left_c.map(|c| c.face_idx());
                if self.is_right_face_visited(c) {
                    if let Some(right_face) = maybe_right_face {
                        self.check_and_store_topology_split_event(
                            self.last_encoded_symbol_idx,
                            Orientation::Right,
                            right_face,
                        );
                    }
                    if self.is_left_face_visited(c) {
                        // 'E' symbol
                        if let Some(left_face) = maybe_left_face {
                            self.check_and_store_topology_split_event(
                                self.last_encoded_symbol_idx,
                                Orientation::Left,
                                left_face,
                            );
                        }
                        self.traversal.record_symbol(
                            Symbol::E,
                            &self.visited_faces,
                            self.pos_corner_table,
                        );
                        self.corner_traversal_stack.pop();
                        // End of a branch of the traversal.
                        break;
                    } else {
                        // 'R' symbol
                        self.traversal.record_symbol(
                            Symbol::R,
                            &self.visited_faces,
                            self.pos_corner_table,
                        );
                        c = maybe_left_c.unwrap(); // unwrap is safe here; we checked that the left face is not visited, which implies that the left face exist.
                    }
                } else if self.is_left_face_visited(c) {
                    // 'L' symbol
                    if let Some(left_face) = maybe_left_face {
                        self.check_and_store_topology_split_event(
                            self.last_encoded_symbol_idx,
                            Orientation::Left,
                            left_face,
                        );
                    }
                    self.traversal.record_symbol(
                        Symbol::L,
                        &self.visited_faces,
                        self.pos_corner_table,
                    );
                    c = maybe_right_c.unwrap(); // unwrap is safe here; we checked that the right face is not visited, which implies that the right face exist.
                } else {
                    self.traversal.record_symbol(
                        Symbol::S,
                        &self.visited_faces,
                        self.pos_corner_table,
                    );
                    self.num_split_symbols += 1;
                    if let Some(hole_idx) = self.vertex_hole_id[v] {
                        if !self.visited_holes[hole_idx] {
                            self.process_boundary(c, false);
                        }
                    }
                    self.face_to_split_symbol_map[face_idx] = self.last_encoded_symbol_idx as u32;
                    *self.corner_traversal_stack.last_mut().unwrap() = maybe_left_c.unwrap();
                    self.corner_traversal_stack.push(maybe_right_c.unwrap());
                    break;
                }
            }
        }
        Ok(())
    }

    /// Checks whether the right face of the corner 'c' is visited.
    /// If the corner is on a boundary and if the right face does not exist,
    /// then it returns true by convention.
    fn is_right_face_visited(&self, c: CornerIdx) -> bool {
        if let Some(c_r) = self.posds.corner_table().get_right_corner(c) {
            self.visited_faces[c_r.face_idx()]
        } else {
            true
        }
    }

    /// Checks whether the left face of the corner 'c' is visited.
    /// If the corner is on a boundary and if the left face does not exist,
    /// then it returns true by convention.
    fn is_left_face_visited(&self, c: CornerIdx) -> bool {
        if let Some(c_l) = self.pos_corner_table.get_left_corner(c) {
            self.visited_faces[c_l.face_idx()]
        } else {
            true
        }
    }

    fn encode_topology_splits<W>(&mut self, writer: &mut W) -> Result<(), Err>
    where
        W: ByteWriter,
    {
        let mut last_idx = 0;
        // write the number of topology splits.
        leb128_write(self.topology_splits.len() as u64, writer);
        for split in self.topology_splits.iter() {
            leb128_write((split.merging_symbol_idx - last_idx) as u64, writer);
            leb128_write(
                (split.merging_symbol_idx - split.split_symbol_idx) as u64,
                writer,
            );
            last_idx = split.merging_symbol_idx;
        }
        let mut bit_coder: BitWriter<'_, W, LsbFirst> = BitWriter::spown_from(writer);
        for split in self.topology_splits.iter() {
            let orientation = match split.merging_edge_orientation {
                Orientation::Left => (1, 0),
                Orientation::Right => (1, 1),
            };
            bit_coder.write_bits(orientation);
        }
        Ok(())
    }

    /// Begins the Edgebreaker iteration from the given face.
    /// The first boolean indicates whether the face is interior (i.e. the face does not touch a boundary) or not.
    /// The second 'usize' element is a corner chosen as follows:
    /// It chooses the first corner of the face as the starting point is such a way that corner faces the the boundary
    /// if the face is on the boundary.
    /// If the face is not on the boundary, then it returns the input corner.
    fn begin_from(&mut self, face_idx: FaceIdx) -> (bool, CornerIdx) {
        let mut corner_index = CornerIdx::from(3 * usize::from(face_idx));
        for _ in 0..3 {
            if self.pos_corner_table.opposite(corner_index).is_none() {
                // corner faces a boundary
                return (false, corner_index);
            }
            if self.vertex_hole_id[self.posds.vertex_idx(corner_index)].is_some() {
                // The corner is on a boundary.
                while let Some(right_corner) = self.posds.corner_table().swing_right(corner_index) {
                    corner_index = right_corner;
                }
                let start_corner = corner_index.previous();
                return (false, start_corner);
            }
            corner_index = corner_index.next();
        }
        (true, corner_index)
    }

    fn check_and_store_topology_split_event(
        &mut self,
        merging_symbol_idx: usize,
        merging_edge_orientation: Orientation,
        split_face_idx: FaceIdx,
    ) {
        let split_symbol_idx = self.face_to_split_symbol_map[split_face_idx];
        if split_symbol_idx == u32::MAX {
            return;
        }
        let split = TopologySplit {
            merging_symbol_idx,
            split_symbol_idx: split_symbol_idx as usize,
            merging_edge_orientation,
        };

        self.topology_splits.push(split);
    }
}

impl<'ads, 'faces, T> ConnectivityEncoder for Edgebreaker<'ads, 'faces, T>
where
    T: Traversal,
{
    type Config = Config;
    type Err = Err;
    /// The main encoding paradigm for Edgebreaker.
    ///
    /// Returns the corners of the edgebreaker traversal (`corners_of_edgebreaker`), i.e. the
    /// last-encoded corner of each connected component in encoded order. This ordering seeds the
    /// per-attribute sequencing (`Traverser`) during attribute encoding, so it must be surfaced
    /// back to the caller.
    fn encode_connectivity<W>(mut self, writer: &mut W) -> Result<Vec<CornerIdx>, Self::Err>
    where
        W: ByteWriter,
    {
        debug_write!("Init Decoder", writer);
        // encode the traversal decoder type
        self.config.traversal.write_to(writer);
        debug_write!("Init Decoder Done", writer);

        self.compute_boundaries()?;

        leb128_write(self.posds.num_vertices() as u64, writer);
        leb128_write(self.gds.num_faces() as u64, writer);

        writer.write_u8((self.adss.len() - 1) as u8);

        // Run Edgebreaker once for each connected component.
        for c in 0..self.gds.num_corners() {
            let c = CornerIdx::from(c);
            let face_idx = c.face_idx();
            if self.visited_faces[face_idx] {
                // if the face is already visited, then skip it.
                continue;
            }

            let (is_start_face_interior, start_corner) = self.begin_from(face_idx);

            self.traversal
                .record_start_face_config(is_start_face_interior);

            if is_start_face_interior {
                let corner_index = start_corner;
                let v = self.posds.vertex_idx(corner_index);
                let n = self.posds.vertex_idx(corner_index.next());
                let p = self.posds.vertex_idx(corner_index.previous());
                self.visited_vertices[v] = true;
                self.visited_vertices[n] = true;
                self.visited_vertices[p] = true;

                self.vertex_traversal_length.push(1);

                self.visited_faces[face_idx] = true;

                self.init_face_connectivity_corners
                    .push(corner_index.next());
                let corner_opp = self.pos_corner_table.opposite(corner_index.next()).unwrap(); // the face is interior, so every edge has an opposite corner
                self.edgebreaker_from(corner_opp)?;
            } else {
                // if the face is on the boundary, then we start from the boundary.
                self.process_boundary(start_corner.next(), true);
                self.edgebreaker_from(start_corner)?;
            }
        }

        // write the number of symbols.
        leb128_write(self.traversal.num_symbols() as u64, writer);

        // write the number of encoded split symbols.
        leb128_write(self.num_split_symbols as u64, writer);

        self.encode_topology_splits(writer)?;
        // encode the edgebreaker symbols.
        self.traversal
            .encode(writer, self.adss, self.pos_corner_table, self.gds)?;

        self.init_face_connectivity_corners.reverse();
        self.init_face_connectivity_corners
            .append(&mut self.processed_connectivity_corners);

        Ok(self.init_face_connectivity_corners)
    }
}

pub(crate) trait Traversal {
    fn record_symbol(
        &mut self,
        symbol: Symbol,
        visited_faces: &VecFaceIdx<bool>,
        corner_table: &CornerTable,
    );
    fn record_start_face_config(&mut self, interior_cfg: bool);
    fn new_corner_reached(&mut self, corner: CornerIdx);
    fn num_symbols(&self) -> usize;
    fn encode<W>(
        self,
        writer: &mut W,
        att_data: &[AttributeDS<'_>],
        corner_table: &CornerTable,
        gds: &DS,
    ) -> Result<(), Err>
    where
        W: ByteWriter;
}

pub(crate) struct DefaultTraversal {
    symbols: Vec<Symbol>,
    interior_cfg: Vec<bool>,
    processed_connectivity_corners: Vec<CornerIdx>,
}

impl DefaultTraversal {
    pub(crate) fn new() -> Self {
        Self {
            symbols: Vec::new(),
            interior_cfg: Vec::new(),
            processed_connectivity_corners: Vec::new(),
        }
    }
}

impl Traversal for DefaultTraversal {
    fn record_symbol(
        &mut self,
        symbol: Symbol,
        _visited_faces: &VecFaceIdx<bool>,
        _corner_table: &CornerTable,
    ) {
        self.symbols.push(symbol);
    }

    fn new_corner_reached(&mut self, corner: CornerIdx) {
        self.processed_connectivity_corners.push(corner);
    }

    fn record_start_face_config(&mut self, interior_cfg: bool) {
        self.interior_cfg.push(interior_cfg);
    }

    fn num_symbols(&self) -> usize {
        self.symbols.len()
    }

    fn encode<W>(
        self,
        final_writer: &mut W,
        att_data: &[AttributeDS<'_>],
        pos_corner_table: &CornerTable,
        gds: &DS,
    ) -> Result<(), Err>
    where
        W: ByteWriter,
    {
        let mut writer = Vec::new();
        {
            let mut writer: BitWriter<'_, Vec<u8>, LsbFirst> = BitWriter::spown_from(&mut writer);
            for s in self.symbols.into_iter().rev() {
                writer.write_bits(CrLight::encode_symbol(s));
            }
        }

        // encode the size
        leb128_write(writer.len() as u64, final_writer);
        // write the encoded symbols.
        for byte in writer {
            final_writer.write_u8(byte);
        }

        encode_start_faces(&self.interior_cfg, final_writer)?;
        encode_attribute_seams(
            self.processed_connectivity_corners,
            att_data,
            pos_corner_table,
            gds,
            final_writer,
        )
    }
}

/// Encodes the start-face interior flags as a rabs sub-stream
/// (`[prob_zero | leb128 len | bytes]`), bits written in reverse.
fn encode_start_faces<W>(interior_cfg: &[bool], final_writer: &mut W) -> Result<(), Err>
where
    W: ByteWriter,
{
    let freq_count_0 = interior_cfg.iter().filter(|&&cfg| !cfg).count();
    // the probability of zero in [0,1] is scaled to [0,256], and clamped to [1,255] as the rans does not accept the zero probability.
    let zero_prob = (((freq_count_0 as f32 / interior_cfg.len() as f32) * 256.0 + 0.5) as u16)
        .clamp(1, 255) as u8;
    final_writer.write_u8(zero_prob);
    let mut writer: RabsCoder = RabsCoder::new(zero_prob as usize, None);
    for &cfg in interior_cfg.iter().rev() {
        writer.write(if cfg { 1 } else { 0 })?;
    }
    let buffer = writer.flush()?;
    leb128_write(buffer.len() as u64, final_writer);
    for byte in buffer {
        final_writer.write_u8(byte);
    }
    Ok(())
}

/// Encodes the per-attribute seam bits, one rabs sub-stream per non-position
/// attribute, in the face order the decoder reconstructs (the processed corners
/// reversed).
///
/// A single walk packs every stream's flag for an edge into one byte and counts
/// each stream's zeros, so the sub-stream probabilities need no second pass; the
/// coders then all run over one reverse pass of those bytes. This mirrors the
/// decoder, which unpacks the same byte layout with all its rabs decoders live
/// at once.
///
/// Seams are encoded per non-position attribute only: the position attribute
/// defines the base connectivity and carries no seams. This must match the
/// `adss.len() - 1` attribute count written in `encode_connectivity`; including
/// the position attribute here would emit one extra seam stream and desync the
/// decoder.
fn encode_attribute_seams<W>(
    processed_connectivity_corners: Vec<CornerIdx>,
    att_data: &[AttributeDS<'_>],
    pos_corner_table: &CornerTable,
    gds: &DS,
    final_writer: &mut W,
) -> Result<(), Err>
where
    W: ByteWriter,
{
    let seam_atts = att_data
        .iter()
        .filter(|ads| ads.att_data().get_attribute_type() != AttributeType::Position)
        .collect::<Vec<_>>();
    // The flags of up to eight streams pack into one byte, matching how the
    // decoder unpacks them; a mesh carrying more seam attributes than that takes
    // one walk per group of eight.
    for group in seam_atts.chunks(8) {
        let mut visited_faces = vec![false; gds.num_faces()];
        let mut packed: Vec<u8> = Vec::with_capacity(gds.num_corners() >> 1);
        let mut zeros = vec![0usize; group.len()];
        for c in processed_connectivity_corners.iter().rev().copied() {
            let corners = [c, c.next(), c.previous()];
            let f_idx = c.face_idx();
            visited_faces[usize::from(f_idx)] = true;
            for corner in &corners {
                if let Some(opp_corner) = pos_corner_table.opposite(*corner) {
                    let opp_face = opp_corner.face_idx();
                    if visited_faces[usize::from(opp_face)] {
                        // if the opposite face is already visited, then we do not need to record the attribute seam.
                        continue;
                    }
                } else {
                    // if the edge opposite to the corner is on a boundary, then we do not need to record the attribute seam.
                    continue;
                }

                let mut bits = 0u8;
                for (j, ads) in group.iter().enumerate() {
                    if ads.corner_table().opposite(*corner).is_none() {
                        bits |= 1 << j;
                    } else {
                        zeros[j] += 1;
                    }
                }
                packed.push(bits);
            }
        }
        write_seam_streams(&packed, &zeros, final_writer)?;
    }

    Ok(())
}

/// The zero probability of a seam sub-stream, scaled from `[0,1]` to `[0,256]`
/// and clamped to `[1,255]` as rans rejects a zero probability.
fn seam_prob_zero(zeros: usize, total: usize) -> u8 {
    (((zeros as f32 / total as f32) * 256.0 + 0.5) as u16).clamp(1, 255) as u8
}

/// Emits one rabs sub-stream per packed stream, in stream order, each as
/// `[prob_zero | leb128 len | bytes]`. Every stream's coder runs concurrently
/// over a single reverse pass of `packed`, mirroring the decoder's concurrent
/// seam decode; bits go out reversed because rabs decodes in the order opposite
/// to encoding.
fn write_seam_streams<W>(packed: &[u8], zeros: &[usize], final_writer: &mut W) -> Result<(), Err>
where
    W: ByteWriter,
{
    let probs: Vec<u8> = zeros
        .iter()
        .map(|&z| seam_prob_zero(z, packed.len()))
        .collect();
    let buffers = match probs.len() {
        1 => encode_seams_fixed::<1>(packed, &probs),
        2 => encode_seams_fixed::<2>(packed, &probs),
        _ => encode_seams_general(packed, &probs),
    }?;
    for (prob, buffer) in probs.iter().zip(buffers) {
        final_writer.write_u8(*prob);
        leb128_write(buffer.len() as u64, final_writer);
        for byte in buffer {
            final_writer.write_u8(byte);
        }
    }
    Ok(())
}

/// The concurrent seam encode monomorphized on the stream count, so the coder
/// states stay in locals.
fn encode_seams_fixed<const N: usize>(packed: &[u8], probs: &[u8]) -> Result<Vec<Vec<u8>>, Err> {
    let mut coders: [RabsCoder; N] =
        std::array::from_fn(|j| RabsCoder::new(probs[j] as usize, None));
    for &bits in packed.iter().rev() {
        for (j, coder) in coders.iter_mut().enumerate() {
            coder.write((bits >> j) & 1)?;
        }
    }
    let mut buffers = Vec::with_capacity(coders.len());
    for coder in coders {
        buffers.push(coder.flush()?);
    }
    Ok(buffers)
}

/// Fallback encode for stream counts without a monomorphization.
fn encode_seams_general(packed: &[u8], probs: &[u8]) -> Result<Vec<Vec<u8>>, Err> {
    let mut coders: Vec<RabsCoder> = probs
        .iter()
        .map(|&prob| RabsCoder::new(prob as usize, None))
        .collect();
    for &bits in packed.iter().rev() {
        for (j, coder) in coders.iter_mut().enumerate() {
            coder.write((bits >> j) & 1)?;
        }
    }
    let mut buffers = Vec::with_capacity(coders.len());
    for coder in coders {
        buffers.push(coder.flush()?);
    }
    Ok(buffers)
}

pub(crate) struct ValenceTraversal {
    /// Valence of the not-yet-encoded part of the mesh per vertex. Signed to
    /// tolerate transient negative values on malformed inputs, as in Google's
    /// reference implementation.
    vertex_valences: VecVertexIdx<isize>,
    /// Per-corner vertex, diverging from the position DS as S symbols split
    /// vertices.
    corner_to_vertex_map: VecCornerIdx<VertexIdx>,
    context_symbols: Vec<Vec<Symbol>>,
    last_corner: CornerIdx,
    prev_symbol: Option<Symbol>,
    interior_cfg: Vec<bool>,
    num_symbols: usize,
    processed_connectivity_corners: Vec<CornerIdx>,
}
impl ValenceTraversal {
    #[inline]
    fn vertex_idx(&self, corner: CornerIdx) -> VertexIdx {
        self.corner_to_vertex_map[corner]
    }

    pub(crate) fn new(pos_ds: &AttributeDS) -> Self {
        let mut vertex_valences: VecVertexIdx<isize> =
            Vec::with_capacity(pos_ds.num_vertices()).into();
        for i in 0..pos_ds.num_vertices() {
            let v = VertexIdx::from(i);
            vertex_valences.push(pos_ds.vertex_valence(v) as isize);
        }

        let num_corners = pos_ds.global_ds().num_corners();
        let mut corner_to_vertex_map: VecCornerIdx<VertexIdx> =
            Vec::with_capacity(num_corners).into();
        for c in 0..num_corners {
            corner_to_vertex_map.push(pos_ds.vertex_idx(CornerIdx::from(c)));
        }

        let num_unique_valences = MAX_VALENCE - MIN_VALENCE + 1;

        let context_symbols = vec![Vec::new(); num_unique_valences];
        Self {
            vertex_valences,
            corner_to_vertex_map,
            context_symbols,
            last_corner: CornerIdx::INVALID, // This will be set to a valid corner index in `new_corner_reached` before the first call to record symbol.
            prev_symbol: None,
            interior_cfg: Vec::new(),
            num_symbols: 0,
            processed_connectivity_corners: Vec::new(),
        }
    }
}

impl Traversal for ValenceTraversal {
    fn record_symbol(
        &mut self,
        symbol: Symbol,
        visited_faces: &VecFaceIdx<bool>,
        corner_table: &CornerTable,
    ) {
        self.num_symbols += 1;

        let next = self.last_corner.next();
        let prev = self.last_corner.previous();

        let v_last = self.vertex_idx(self.last_corner);
        let v_next = self.vertex_idx(next);
        let v_prev = self.vertex_idx(prev);

        let active_valence = self.vertex_valences[v_next];
        match symbol {
            Symbol::C | Symbol::S => {
                self.vertex_valences[v_next] -= 1;
                self.vertex_valences[v_prev] -= 1;
            }
            Symbol::R => {
                // Update valences.
                self.vertex_valences[v_last] -= 1;
                self.vertex_valences[v_next] -= 1;
                self.vertex_valences[v_prev] -= 2;
            }
            Symbol::L => {
                self.vertex_valences[v_last] -= 1;
                self.vertex_valences[v_next] -= 2;
                self.vertex_valences[v_prev] -= 1;
            }
            Symbol::E => {
                self.vertex_valences[v_last] -= 2;
                self.vertex_valences[v_next] -= 2;
                self.vertex_valences[v_prev] -= 2;
            }
        }
        if symbol == Symbol::S {
            // The decoder merges the split vertex only when it processes the S
            // symbol (it decodes in reverse), so the vertex is split here: the
            // left side keeps the vertex with the valence of the still
            // unencoded left fan, and the corners of the right fan are
            // remapped to a fresh vertex carrying the right-fan valence.
            let mut num_left_faces = 0;
            let mut maybe_act_c = corner_table.opposite(prev);
            while let Some(act_c) = maybe_act_c {
                if visited_faces[act_c.face_idx()] {
                    break;
                }
                num_left_faces += 1;
                maybe_act_c = corner_table.opposite(act_c.next());
            }
            self.vertex_valences[v_last] = num_left_faces + 1;

            let new_vertex = self.vertex_valences.len();
            let mut num_right_faces = 0;

            maybe_act_c = corner_table.opposite(next);
            while let Some(act_c) = maybe_act_c {
                if visited_faces[act_c.face_idx()] {
                    break;
                }
                num_right_faces += 1;
                self.corner_to_vertex_map[act_c.next()] = new_vertex.into();
                maybe_act_c = corner_table.opposite(act_c.previous());
            }
            self.vertex_valences.push(num_right_faces + 1);
        }

        if let Some(prev_symbol) = self.prev_symbol {
            let clamped_valence = active_valence.clamp(MIN_VALENCE as isize, MAX_VALENCE as isize);

            let context = (clamped_valence - MIN_VALENCE as isize) as usize;
            self.context_symbols[context].push(prev_symbol);
        }

        self.prev_symbol = Some(symbol);
    }

    fn record_start_face_config(&mut self, interior_cfg: bool) {
        self.interior_cfg.push(interior_cfg);
    }

    fn new_corner_reached(&mut self, c: CornerIdx) {
        self.last_corner = c;
        self.processed_connectivity_corners.push(c);
    }

    fn num_symbols(&self) -> usize {
        self.num_symbols
    }

    fn encode<W>(
        self,
        writer: &mut W,
        att_data: &[AttributeDS<'_>],
        pos_corner_table: &CornerTable,
        gds: &DS,
    ) -> Result<(), Err>
    where
        W: ByteWriter,
    {
        encode_start_faces(&self.interior_cfg, writer)?;
        encode_attribute_seams(
            self.processed_connectivity_corners,
            att_data,
            pos_corner_table,
            gds,
            writer,
        )?;

        // Store the contexts.
        for context in self.context_symbols {
            leb128_write(context.len() as u64, writer);
            if context.is_empty() {
                continue;
            }
            let context = context
                .iter()
                .map(|&s| s.get_id() as u64)
                .collect::<Vec<_>>();

            encode_symbols(context, 1, SymbolEncodingMethod::DirectCoded, writer)?;
        }

        Ok(())
    }
}
