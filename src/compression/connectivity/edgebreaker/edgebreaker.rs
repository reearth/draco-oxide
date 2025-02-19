// // Define the edgebreaker compression algorithm

// pub struct EdgeBreaker<'a, CoordValType> {
// 	/// 'points' is the mutable reference to the coordinate values for the vertices
// 	/// of the input mesh. At the end of the encoding process, they are reordered
// 	/// in a way that they are encoded.
// 	points: &'a mut [[[CoordValType;3]; 3]],
	
// 	/// 'edges' is a set of edges of the input mesh, each of which is a two-element 
// 	/// non-multiset sorted in the increasing order. 'edges' itself is also sorted 
// 	/// by the initial vertex of its edges in the increasing order.
// 	edges: Vec<[VertexId;2]>,
	
// 	/// 'coboundary_map' records the coboundary information of edges, i.e. the i'th 
// 	/// entry of this array stores the indexes of the faces that has 'edge[i]'
// 	/// as the boundary. Since the mesh is pre-processed to be homeomorphic to 
// 	/// manifolds with boundaries, there are exactly one or two coboundary faces 
// 	/// for each edge.
// 	coboundary_map: Vec<(FaceIdx, Option<FaceIdx>)>,
	
// 	/// 'faces' is a set of faces of the mesh. Just like edges, each face is a 
// 	/// sorted three-element non-multiset, and 'faces' itself is sorted.
// 	faces: Vec<[VertexId;3]>,
	
// 	/// The 'i'th entry of 'visited_vertices' is true if the Edgebreaker has
// 	/// already visited the 'i' th vertex.
// 	visited_vertices: Vec<bool>,
	
// 	/// 'boundary_stack' is used to remember the splits of the traversal, i.e. 
// 	/// this array is pushed when 'S' symbol is recorded and is popped when
// 	/// 'E' sybol is recorded.
// 	boundary_stack: Vec<Vec<VertexIdx>>,
	
// 	/// This represents the active boundary. 'i'th vertex and the 'i+1'th vertex,
// 	/// as well as the first vertex and the last vertex, are connected by an edge,
// 	/// forming a boundary homeomorphic to a circle.
// 	active_edge_stack: Vec<[VertexIdx;2]>,
	
// 	/// configurations for the encoder
// 	config: Config
// }

// impl<CoordValType: Float> EdgeBreaker<CoordValType> {
// 	// Build the object with empty arrays.
// 	pub fn new()->Self;
	
// 	/// Initializes the edgebreaker. This function takes in a mesh and 
// 	/// decomposes it into manifolds with boundaries if it is not homeomorhic to a
// 	/// manifold. 
// 	pub(crate) fn init(mesh: Mesh, config: option<Condfig>) -> Self {
// 		let (edges, faces, coboundary_map, num_points) = Self::decompose_into_manifolds(
// 			mesh.get_faces(), 
// 			num_vertices
// 		);
//         let points = mesh.get_points_mut();
// 		let visited_vertices = [false; vertices.len()];
//         let config = config.unwrap_or(Config::default());
// 		Self{
//             points,
// 			edges, 
// 			faces, 
// 			coboundary_map,
// 			visited_vertices,
//             config,
//             boundary_stack: Vec::new(),
//             active_edge_stack: Vec::new(),
// 		}
// 	}

// 	/// A function that decomposes the mesh so that it is a manifold with boundary.
// 	/// The worst time complexity of this is O(N*log(N)).
// 	/// This function is used in the constructor 'Self::new()'. 
// 	/// This function first iterates over the 
// 	/// faces and creates 'edges: Vec<<VertexId;2>>' and 'coboundary_map:
// 	/// Vec<(FaceIdx, Option<FaceIdx>>' (The time complexity is O(faces.len())). 
// 	/// We then find the edges breaking the manifold condition by looking at the
// 	/// cardinality of each entry of 'coboundary_map', and modifies these variables
// 	/// so that no three faces share an edge. (The time complexity is n*log(n),
// 	/// where n=edges.len()). Finally, we disconnect the meshes at the vertices that
// 	/// violate the manifold condition. This can be achieved again by looking at 
// 	/// 'coboundary_map'; if a vertex has too many coboundary edges that are
// 	/// boundaries of the mesh, then it must be an edge. The time complexity of this
// 	/// is O(n), where n=max(edges.len(), n_vertices).
// 	fn decompose_into_manifolds(faces: &[[VertexIdx;3]], num_points: usize) 
// 		-> Result<(
// 				Vec<[VertexId;2]>, 
// 				Vec<[VertexId;3]>, 
// 				Vec<(FaceIdx, Option<FaceIdx>)>,
// 				usize
// 			), Err>;
	
	
// 	/// A function implementing the recursive step of the edgebreaker algorithm.
// 	/// When this function returns, all the CLERS symbols are written in the
// 	/// buffer in the reverse order. Since the time complexity of 'find_vertices_pinching()' 
// 	/// is O(1), the complexity of this function (single recursive step) is also O(1).
// 	fn edge_breaker_recc(buffer: &mut EncoderBuffer) -> Result<(), Err>;
	
// 	// This function finds the vertices that are "pinching" the given edge, that is,
// 	// it returns a vertex disjoint from the edge such that there is a face 
// 	// containing both the vertex and the edge. There is exactly one or two such 
// 	// vertices for each edge since the mesh satisfies the manifold condition. 
// 	// The time complexity of this function is O(1), since we have the 'coboundary_map'.
// 	fn find_vertices_pinching(self, edge: EdgeIdx) -> (VertexIdx, Option<VertexIdx>);
// }	

// // impl ConnectivityEncoder for Edgebreaker {
// // 	type Err = Err;
// // 	/// The main encoding paradigm for an edge breaker.
// // 	fn encode(self, buffer: &mut EncoderBuffer) -> Result<(), Self::Err> {
// // 		// Run Edgebreaker once for each connected component.
// // 		while let Some(v) = self.get_some_unvisited_vertex() {
// // 			let mut t = self.get_some_triangle_containing(v).into_vec();
// // 			self.active_edge_stack.push([t[0], t[1]]);
// // 			self.active_boundary_stack.push(t);
// // 			self.edge_breaker_recc(buffer, t, [t[0], t[1]]);
// // 		}
// // 	}
// // }