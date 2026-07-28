use std::{cmp, fmt};

use crate::encode::entropy::symbol_coding::encode_symbols;
use draco_oxide_core::attribute::AttributeType;
use draco_oxide_core::bit_coder::{BitWriter, ByteWriter};
use draco_oxide_core::buffer::LsbFirst;
use draco_oxide_core::codec::connectivity::edgebreaker::symbol_encoder::{
    CrLight, Symbol, SymbolEncoder,
};
use draco_oxide_core::codec::entropy::rans::{self, RabsCoder};
use draco_oxide_core::debug_write;
use draco_oxide_core::mesh::ds::CornerTable;
use draco_oxide_core::mesh::ds::GenericCornerTable;
use draco_oxide_core::mesh::ds::{AttributeDS, DS};

use draco_oxide_core::types::{
    ConfigType, CornerIdx, FaceIdx, VecFaceIdx, VecVertexIdx, VertexIdx,
};

use draco_oxide_core::codec::connectivity::edgebreaker::{
    self, EdgebreakerKind, Orientation, TopologySplit, MAX_VALENCE, MIN_VALENCE,
};
use draco_oxide_core::codec::entropy::SymbolEncodingMethod;
use draco_oxide_core::utils::bit_coder::leb128_write;
use std::collections::BTreeMap;
use std::vec;

use crate::encode::connectivity::ConnectivityEncoder;

#[cfg(feature = "evaluation")]
use crate::eval;

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

    face_to_split_symbol_map: BTreeMap<usize, usize>,

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

#[derive(Clone, fmt::Debug, cmp::PartialEq)]
pub struct Config {
    pub traversal: EdgebreakerKind,
    pub use_single_connectivity: bool,
}

impl ConfigType for Config {
    fn default() -> Self {
        Self {
            traversal: EdgebreakerKind::Valence,
            use_single_connectivity: false,
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
            face_to_split_symbol_map: BTreeMap::new(),
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
                    self.face_to_split_symbol_map
                        .insert(usize::from(face_idx), self.last_encoded_symbol_idx);
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
        #[cfg(feature = "evaluation")]
        {
            let mut string = String::new();
            for split in self.topology_splits.iter() {
                string.push_str(&format!(
                    "{}:{}({:?}) ",
                    split.merging_symbol_idx,
                    split.split_symbol_idx,
                    split.merging_edge_orientation
                ));
            }
            eval::write_json_pair("topology_splits", serde_json::Value::from(string), writer);
        }
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
        let split_symbol_idx = if let Some(&idx) = self
            .face_to_split_symbol_map
            .get(&usize::from(split_face_idx))
        {
            idx
        } else {
            // The face is not split, so we do not need to store the split event.
            return;
        };
        let split = TopologySplit {
            merging_symbol_idx,
            split_symbol_idx,
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
    let mut visited_faces = vec![false; gds.num_faces()];
    let mut seams_data = (0..seam_atts.len())
        .map(|_| Vec::with_capacity(gds.num_corners() >> 1))
        .collect::<Vec<_>>();
    for c in processed_connectivity_corners.into_iter().rev() {
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

            for (j, ads) in seam_atts.iter().enumerate() {
                // store true if the corner is on an attribute seam, false otherwise.
                seams_data[j].push(ads.corner_table().opposite(*corner).is_none());
            }
        }
    }
    // encode the attribute seams.
    for seams_data in seams_data {
        let freq_count_0 = seams_data.iter().filter(|&&s| !s).count();
        let prob_zero = (((freq_count_0 as f32 / seams_data.len() as f32) * 256.0 + 0.5) as u16)
            .clamp(1, 255) as u8;
        final_writer.write_u8(prob_zero);
        let mut writer: RabsCoder = RabsCoder::new(prob_zero as usize, None);
        for &s in seams_data.iter().rev() {
            writer.write(if s { 1 } else { 0 })?;
        }
        let buffer = writer.flush()?;
        leb128_write(buffer.len() as u64, final_writer);
        for byte in buffer {
            final_writer.write_u8(byte);
        }
    }

    Ok(())
}

pub(crate) struct ValenceTraversal<'pos_ds> {
    /// Valence of the not-yet-encoded part of the mesh per vertex. Signed to
    /// tolerate transient negative values on malformed inputs, as in Google's
    /// reference implementation.
    vertex_valences: VecVertexIdx<isize>,
    pos_ds: &'pos_ds AttributeDS<'pos_ds>,
    diff_corner_to_vertex_map: BTreeMap<CornerIdx, VertexIdx>,
    context_symbols: Vec<Vec<Symbol>>,
    last_corner: CornerIdx,
    prev_symbol: Option<Symbol>,
    interior_cfg: Vec<bool>,
    num_symbols: usize,
    processed_connectivity_corners: Vec<CornerIdx>,
}
impl<'pos_ds> ValenceTraversal<'pos_ds> {
    fn vertex_idx(&self, corner: CornerIdx) -> VertexIdx {
        if let Some(&vertex) = self.diff_corner_to_vertex_map.get(&corner) {
            vertex
        } else {
            self.pos_ds.vertex_idx(corner)
        }
    }

    pub(crate) fn new(pos_ds: &'pos_ds AttributeDS) -> Self {
        let mut vertex_valences: VecVertexIdx<isize> =
            Vec::with_capacity(pos_ds.num_vertices()).into();
        for i in 0..pos_ds.num_vertices() {
            let v = VertexIdx::from(i);
            vertex_valences.push(pos_ds.vertex_valence(v) as isize);
        }

        let num_unique_valences = MAX_VALENCE - MIN_VALENCE + 1;

        let context_symbols = vec![Vec::new(); num_unique_valences];
        Self {
            vertex_valences,
            pos_ds,
            diff_corner_to_vertex_map: BTreeMap::new(),
            context_symbols,
            last_corner: CornerIdx::INVALID, // This will be set to a valid corner index in `new_corner_reached` before the first call to record symbol.
            prev_symbol: None,
            interior_cfg: Vec::new(),
            num_symbols: 0,
            processed_connectivity_corners: Vec::new(),
        }
    }
}

impl<'pos_ds> Traversal for ValenceTraversal<'pos_ds> {
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
                self.diff_corner_to_vertex_map
                    .insert(act_c.next(), new_vertex.into());
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

// // #[cfg(not(feature = "evaluation"))]
// #[cfg(test)]
// mod tests {
//     use std::vec;

//     use draco_oxide_core::attribute::AttributeId;
//     use draco_oxide_core::types::Vector;
//     use draco_oxide_core::types::NdVector;
//     use crate::debug_expect;
//     use crate::prelude::{BitReader, ByteReader};
//     use draco_oxide_core::codec::connectivity::eq;
//     use draco_oxide_core::utils::bit_coder::leb128_read;

//     use super::*;

//     // #[test]
//     #[allow(unused)]
//     fn test_decompose_into_manifolds_simple() {
//         let mut faces = vec![
//             [0, 1, 6], // 0
//             [1, 6, 7], // 1
//             [2, 3, 6], // 2
//             [3, 6, 7], // 3
//             [4, 5, 6], // 4
//             [5, 6, 7], // 5
//         ];
//         let mut edgebreaker = Edgebreaker::new(Config::default());

//         let points = vec![NdVector::<3,f32>::zero(); 8];
//         let mut point_att = Attribute::from(
//             AttributeId::new(0),
//             points,
//             AttributeType::Position,
//             Vec::new()
//         );

//         assert!(edgebreaker.init(&mut [&mut point_att], &mut faces).is_ok());

//         let coboundary_map = edgebreaker.coboundary_map_one;

//         let idx_of = |edge: &[usize; 2]| edgebreaker.edges.binary_search(edge).unwrap();
//         assert_eq!(coboundary_map[idx_of(&[0,1])], vec![0]);
//         assert_eq!(coboundary_map[idx_of(&[0,6])], vec![0]);
//         assert_eq!(coboundary_map[idx_of(&[1,6])], vec![0, 1]);
//         assert_eq!(coboundary_map[idx_of(&[1,7])], vec![1]);
//         assert_eq!(coboundary_map[idx_of(&[6,7])], vec![1,3,5]);
//         assert_eq!(coboundary_map[idx_of(&[2,3])], vec![2]);
//         assert_eq!(coboundary_map[idx_of(&[2,6])], vec![2]);
//         assert_eq!(coboundary_map[idx_of(&[3,6])], vec![2,3]);
//         assert_eq!(coboundary_map[idx_of(&[3,7])], vec![3]);
//         assert_eq!(coboundary_map[idx_of(&[4,5])], vec![4]);
//         assert_eq!(coboundary_map[idx_of(&[4,6])], vec![4]);
//         assert_eq!(coboundary_map[idx_of(&[5,6])], vec![4,5]);
//         assert_eq!(coboundary_map[idx_of(&[5,7])], vec![5]);

//     }

//     // #[test]
//     #[allow(unused)]
//     fn test_compute_edges() {
//         let faces = vec![
//             [0, 1, 6], // 0
//             [1, 6, 7], // 1
//             [2, 3, 6], // 2
//             [3, 6, 7], // 3
//             [4, 5, 6], // 4
//             [5, 6, 7], // 5
//         ];
//         let mut edgebreaker = Edgebreaker::new(Config::default());
//         edgebreaker.lies_on_boundary_or_cutting_path = vec![false; 8];

//         edgebreaker.compute_edges(&faces);

//         assert_eq!( edgebreaker.edges,
//             vec![
//                 [0, 1],
//                 [0, 6],
//                 [1, 6],
//                 [1, 7],
//                 [2, 3],
//                 [2, 6],
//                 [3, 6],
//                 [3, 7],
//                 [4, 5],
//                 [4, 6],
//                 [5, 6],
//                 [5, 7],
//                 [6, 7],
//             ]
//         );

//         assert_eq!( edgebreaker.coboundary_map_one,
//             vec![
//                 vec![0],
//                 vec![0],
//                 vec![0,1],
//                 vec![1],
//                 vec![2],
//                 vec![2],
//                 vec![2,3],
//                 vec![3],
//                 vec![4],
//                 vec![4],
//                 vec![4,5],
//                 vec![5],
//                 vec![1,3,5],
//             ]
//         )
//     }

//     #[test]
//     fn test_check_orientability() {
//         // test1: orientable mesh
//         let faces = vec![
//             [0,1,4],
//             [0,3,4],
//             [1,2,5],
//             [1,4,5],
//             [2,5,6],
//             [3,4,7],
//             [3,7,10],
//             [4,5,7],
//             [5,6,8],
//             [5,7,8],
//             [7,8,9],
//             [7,9,10],
//             [8,9,11],
//             [9,10,11]
//         ];
//         let mut edgebreaker = Edgebreaker::new(Config::default());
//         edgebreaker.lies_on_boundary_or_cutting_path = vec![false; 12];
//         edgebreaker.face_orientation = vec!(false; faces.len());
//         edgebreaker.visited_faces = vec!(false; faces.len());
//         edgebreaker.compute_edges(&faces);
//         assert!(edgebreaker.check_orientability(&faces).is_ok());
//         assert_eq!(edgebreaker.face_orientation, vec![true, false, true, false, false, true, true, true, true, false, true, true, false, false]);

//         // test 2: non-orientable mesh
//         let faces = vec![
//             [0, 1, 3],
//             [0, 1, 4],
//             [0, 2, 3],
//             [0, 4, 5],
//             [2, 3, 5],
//             [2, 4, 5],
//         ];
//         let mut edgebreaker = Edgebreaker::new(Config::default());
//         edgebreaker.lies_on_boundary_or_cutting_path = vec![false; 6];

//         edgebreaker.face_orientation = vec!(false; faces.len());
//         edgebreaker.visited_faces = vec!(false; faces.len());
//         edgebreaker.compute_edges(&faces);
//         assert!(edgebreaker.check_orientability(&faces).is_err());

//         let faces = [
//             [9,12,13], [8,9,13], [8,9,10], [1,8,10], [1,10,11], [1,2,11], [2,11,12], [2,12,13],
//             [8,13,14], [7,8,14], [1,7,8], [0,1,7], [0,1,2], [0,2,3], [2,3,13], [3,13,14],
//             [7,14,15], [6,7,15], [0,6,7], [0,5,6], [0,3,5], [3,4,5], [3,4,14], [4,14,15],
//             [6,12,15], [6,9,12], [5,6,9], [5,9,10], [4,5,10], [4,10,11], [4,11,15], [11,12,15]
//         ];
//         let orientation = vec![
//             false, false, true, true, true, false, true, true,
//             false, false, true, false, true, true, false, true,
//             false, false, true, true, true, true, false, true,
//             true, true, false, false, false, false, false, false
//         ];
//         // sort faces while taping orientation
//         let (faces, orientation) = {
//             let mut zipped = faces.iter().zip(orientation.iter()).collect::<Vec<_>>();
//             zipped.sort_by_key(|f| f.0);
//             let faces = zipped.iter().map(|&(&f, _)| f).collect::<Vec<_>>();
//             let orientation = zipped.iter().map(|&(_, &o)| o).collect::<Vec<_>>();
//             (faces, orientation)
//         };
//         let mut edgebreaker = Edgebreaker::new(Config::default());
//         edgebreaker.lies_on_boundary_or_cutting_path = vec![false; 12];
//         edgebreaker.face_orientation = vec!(false; faces.len());
//         edgebreaker.visited_faces = vec!(false; faces.len());
//         edgebreaker.compute_edges(&faces);
//         assert!(edgebreaker.check_orientability(&faces).is_ok());
//         assert_eq!(edgebreaker.face_orientation, orientation,
//             "orientation is wrong at: {:?}",
//             edgebreaker.face_orientation.iter()
//                 .zip(orientation.iter())
//                 .enumerate()
//                 .filter(|(_, (a,b))| a!=b)
//                 .map(|(i,_)| faces[i])
//                 .collect::<Vec<_>>()
//         );
//     }

//     use Symbol::*;
//     fn read_symbols<R>(reader: &mut R, size: usize) -> Vec<Symbol>
//         where R: ByteReader
//     {
//         let mut out = Vec::new();
//         let mut reader = BitReader::spown_from(reader).unwrap();
//         for _ in 0..size {
//             out.push(
//                 CrLight::decode_symbol(&mut reader)
//             );
//         }
//         out
//     }

//     fn read_topology_splits<R: ByteReader>(reader: &mut R) -> Vec<TopologySplit> {
//         let mut topology_splits = Vec::new();
//         let num_topology_splits = leb128_read(reader).unwrap() as u32;
//         let mut last_idx = 0;
//         for _ in 0..num_topology_splits {
//             let source_symbol_idx = leb128_read(reader).unwrap() as usize + last_idx;
//             let split_symbol_idx = source_symbol_idx - leb128_read(reader).unwrap() as usize;
//             let topology_split = TopologySplit {
//                 source_symbol_idx,
//                 split_symbol_idx,
//                 source_edge_orientation: Orientation::Right, // this value is temporary
//             };
//             topology_splits.push(topology_split);
//             last_idx = source_symbol_idx;
//         }

//         let mut reader: BitReader<_> = BitReader::spown_from(reader).unwrap();
//         for split_mut in topology_splits.iter_mut() {
//             // update the orientation of the topology split.
//             split_mut.source_edge_orientation = match reader.read_bits(1).unwrap() {
//                 0 => Orientation::Left,
//                 1 => Orientation::Right,
//                 _ => unreachable!(),
//             };
//         }

//         topology_splits
//     }

//     fn manual_test<const TEST_ORIENTABILITY: bool>(
//         mut original_faces: Vec<[VertexIdx; 3]>,
//         points: Vec<NdVector<3,f32>>,
//         expected_symbols: Vec<Symbol>,
//         expected_topology_splits: Vec<TopologySplit>,
//         expected_faces: Option<Vec<[VertexIdx; 3]>>
//     ) {
//         // positions do not matter
//         let mut point_att = Attribute::from(
//             AttributeId::new(0),
//             points,
//             AttributeType::Position,
//             Vec::new()
//         );

//         let mut buff_writer = Vec::new();
//         Edgebreaker::new(Config::default()).encode_connectivity(&mut original_faces, &mut [&mut point_att], &mut buff_writer).unwrap();

//         let mut reader = buff_writer.into_iter();

//         assert_eq!(reader.read_u8().unwrap(), 0);
//         assert_eq!(reader.read_u64().unwrap(), original_faces.len() as u64);
//         assert_eq!(expected_topology_splits, read_topology_splits(&mut reader));
//         debug_expect!("Start of Symbols", reader);
//         assert_eq!(expected_symbols, read_symbols(&mut reader, original_faces.len()));

//         if !TEST_ORIENTABILITY {
//             original_faces.iter_mut().for_each(|f| f.sort());
//         }
//         if let Some(expected_faces) = expected_faces  {
//             assert_eq!(original_faces, expected_faces);
//         }
//     }

//     #[test]
//     fn edgebreaker_disc() {
//         let faces = vec![
//             [0,1,4],
//             [0,3,4],
//             [1,2,5],
//             [1,4,5],
//             [2,5,6],
//             [3,4,7],
//             [3,7,10],
//             [4,5,7],
//             [5,6,8],
//             [5,7,8],
//             [7,8,9],
//             [7,9,10],
//             [8,9,11],
//             [9,10,11]
//         ];
//         // positions do not matter
//         let points = vec![NdVector::<3,f32>::zero(); faces.iter().flatten().max().unwrap()+1];

//         let expected_symbols = vec![E,E,S,R,L,R,R,C,C,R,R,R,C,C];

//         let expected_faces = vec![
//             [0,1,2],
//             [1,3,4],
//             [0,3,1],
//             [0,5,3],
//             [0,6,5],
//             [5,6,7],
//             [6,8,7],
//             [0,8,6],
//             [0,2,8],
//             [2,9,8],
//             [2,10,9],
//             [2,11,10],
//             [1,11,2],
//             [1,4,11] // orientation base
//         ];

//         manual_test::<true>(faces, points, expected_symbols, Vec::new(), Some(expected_faces));
//     }

//     #[test]
//     fn edgebreaker_split() {
//         let faces = vec![
//             [0,1,2],
//             [0,2,4],
//             [0,4,5],
//             [2,3,4]
//         ];
//         // positions do not matter
//         let points = vec![NdVector::<3,f32>::zero(); faces.iter().flatten().max().unwrap()+1];

//         let expected_symbols = vec![E,E,S,R];

//         let expected_faces = vec![
//             [0,2,1],
//             [1,4,3],
//             [0,1,3],
//             [0,3,5] // orientation base
//         ];

//         manual_test::<true>(faces, points, expected_symbols, Vec::new(), Some(expected_faces));
//     }

//     #[test]
//     fn edgebreaker_triangle() {
//         let faces = vec![
//             [0,1,3],
//             [1,2,3],
//             [2,3,4],
//             [3,4,5]
//         ];

//         let points = vec![NdVector::<3,f32>::zero(); faces.iter().flatten().max().unwrap()+1];
//         let expected_symbols = vec![E,R,R,L];
//         let expected_faces = vec![
//             [0,2,1],
//             [0,1,3],
//             [0,3,4],
//             [0,4,5] // base
//         ];
//         manual_test::<true>(faces, points, expected_symbols, Vec::new(), Some(expected_faces));
//     }

//     #[test]
//     fn edgebreaker_begin_from_center() {
//         // mesh forming a square whose initial edge is not on the boundary.
//         let mut original_faces = vec![
//             [9,23,24], [8,9,23], [8,9,10], [1,8,10], [1,10,11], [1,2,11], [2,11,12], [2,12,13],
//             [8,22,23], [7,8,22], [1,7,8], [0,1,7], [0,1,2], [0,2,3], [2,3,13], [3,13,14],
//             [7,21,22], [6,7,21], [0,6,7], [0,5,6], [0,3,5], [3,4,5], [3,4,14], [4,14,15],
//             [6,20,21], [6,19,20], [5,6,19], [5,18,19], [4,5,18], [4,17,18], [4,15,17], [15,16,17]
//         ];
//         original_faces.sort();
//         // positions do not matter
//         let points = vec![NdVector::<3,f32>::zero(); original_faces.iter().flatten().max().unwrap()+1];

//         let expected_symbols = vec![E, E, E, S, R, L, R, L, R, R, L, R, S, R, E, S, R, C, R, E, L, S, R, C, C, C, R, C, C, L, S /* hole */, C];
//         let expected_topology_splits = vec![
//             TopologySplit {
//                 source_symbol_idx: 16,
//                 split_symbol_idx: 16,
//                 source_edge_orientation: Orientation::Left,
//             },
//         ];
//         manual_test::<false>(original_faces, points, expected_symbols, expected_topology_splits, None);
//     }

//     #[test]
//     fn edgebreaker_handle() {
//         // create torus in order to test the handle symbol.
//         let mut original_faces = vec![
//             [9,12,13], [8,9,13], [8,9,10], [1,8,10], [1,10,11], [1,2,11], [2,11,12], [2,12,13],
//             [8,13,14], [7,8,14], [1,7,8], [0,1,7], [0,1,2], [0,2,3], [2,3,13], [3,13,14],
//             [7,14,15], [6,7,15], [0,6,7], [0,5,6], [0,3,5], [3,4,5], [3,4,14], [4,14,15],
//             [6,12,15], [6,9,12], [5,6,9], [5,9,10], [4,5,10], [4,10,11], [4,11,15], [11,12,15]
//         ];
//         original_faces.sort();
//         // positions do not matter
//         let points = vec![NdVector::<3,f32>::zero(); original_faces.iter().flatten().max().unwrap()+1];

//         let expected_symbols = vec![E, E, S, R, E, E, S, L, R, S, R, C, S /* handle */, R, C, S /* handle */, R, C, C, R, C, C, R, C, C, C, R, C, C, C, C, C];
//         let expected_topology_splits = vec![
//             TopologySplit {
//                 source_symbol_idx: 31,
//                 split_symbol_idx: 17,
//                 source_edge_orientation: Orientation::Left,
//             },
//             TopologySplit {
//                 source_symbol_idx: 28,
//                 split_symbol_idx: 20,
//                 source_edge_orientation: Orientation::Right,
//             }
//         ];

//         manual_test::<false>(original_faces, points, expected_symbols, expected_topology_splits, None);
//     }

//     // #[test]
//     #[allow(unused)] // uncomment the test to run it. it is commented out as it takes a long time to run.
//     fn connectivity_check_after_vertex_permutation() {
//         let (bunny,_) = tobj::load_obj(
//             format!("../tests/data/punctured_sphere.obj"),
//             &tobj::GPU_LOAD_OPTIONS
//         ).unwrap();
//         let bunny = &bunny[0];
//         let mesh = &bunny.mesh;

//         let faces_original = mesh.indices.chunks(3)
//             .map(|x| [x[0] as usize, x[1] as usize, x[2] as usize])
//             .collect::<Vec<_>>();

//         let mut faces = faces_original.clone();

//         let points = mesh.positions.chunks(3)
//             .map(|x| NdVector::<3,f32>::from([x[0], x[1], x[2]]))
//             .collect::<Vec<_>>();

//         let mut point_att = Attribute::from(AttributeId::new(0), points, AttributeType::Position, Vec::new());
//         let mut edgebreaker = Edgebreaker::new(Config::default());
//         assert!(edgebreaker.init(&mut [&mut point_att], &mut faces).is_ok());
//         let mut writer = Vec::new();
//         assert!(edgebreaker.encode_connectivity(&mut faces, &mut [&mut point_att], &mut writer).is_ok());

//         assert!(eq::weak_eq_by_laplacian(&faces, &faces_original).unwrap());
//     }
// }
