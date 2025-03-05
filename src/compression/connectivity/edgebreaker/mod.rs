mod shared;
mod edgebreaker;
mod spiral_reversi;
mod decompose_into_manifolds;
mod prediction;
pub mod symbol_encoder;

use core::fmt;

use symbol_encoder::{
	SymbolEncodingConf,
	Symbol
};

use crate::core::shared::{FaceIdx, EdgeIdx, VertexIdx, ConfigType};

pub(crate) struct EdgeBreaker {
	/// 'edges' is a set of edges of the input mesh, each of which is a two-element 
	/// non-multiset sorted in the increasing order. 'edges' itself is also sorted 
	/// by the initial vertex of its edges in the increasing order.
	edges: Vec<[VertexIdx;2]>,
	
	/// 'coboundary_map_one' records the coboundary information of edges, i.e. the i'th 
	/// entry of this array stores the indexes of the faces that have 'edge[i]'
	/// as the boundary. 
	coboundary_map_one: Vec<Vec<FaceIdx>>,


	/// 'coboundary_map_zero' records the coboundary information of vertices, i.e. the i'th 
	/// entry of this array stores the indexes of the edges that have the 'i'th vertex
	/// as the boundary. This value is lazily computed, and it is 'None' if it is not computed yet.
	/// It is designed to be so because the coboundary information of vertices is necessary only
	/// when the edgebreaker is not homeomorphic to a sphere.
	coboundary_map_zero: Option<Vec<Vec<EdgeIdx>>>,
	
	/// The 'i'th entry of 'visited_vertices' is true if the Edgebreaker has
	/// already visited the 'i' th vertex.
	visited_vertices: Vec<bool>,

	/// The 'i'th entry of 'visited_edges' is true if the Edgebreaker has
	/// already visited the 'i' th face.
	visited_faces: Vec<bool>,
	
	/// This represents the active boundary. 'i'th vertex and the 'i+1'th vertex,
	/// as well as the first vertex and the last vertex, are connected by an edge,
	/// forming a boundary homeomorphic to a circle.
	active_edge_idx_stack: Vec<EdgeIdx>,

	/// This stores the information of the decomposition.
	/// Each element of the vector is a list of vertex indexes that forms a path along a cut.
	cutting_paths: Vec<Vec<VertexIdx>>,

	symbols: Vec<Symbol>,

	/// The orientation of the faces. The 'i'th entry of this array stores the orientation of the 'i'th face.
	face_orientation: Vec<bool>,
	
	/// configurations for the encoder
	config: Config
}

pub struct Config {
    symbol_encoding: SymbolEncodingConf,
}

impl ConfigType for Config {
    fn default() -> Self {
        Self{
			symbol_encoding: SymbolEncodingConf::default()
		}
    }
}

pub enum Err {
    NonOrientable
}

impl fmt::Debug for Err {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::NonOrientable => write!(f, "NonOrientable")
		}
	}
}