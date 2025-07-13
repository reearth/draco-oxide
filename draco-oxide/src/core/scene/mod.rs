use crate::{core::{material::MaterialLibrary, mesh::metadata::Metadata, structural_metadata::StructuralMetadata, texture::TextureLibrary}, Mesh};

type MeshGroupIdx = usize;
type MeshIdx = usize;
type SceneNodeIdx = usize;
type SkinIdx = usize;
type LightIdx = usize;
type InstanceArrayIdx = usize;

// Placeholder math types - these would typically come from a math library like nalgebra
#[derive(Debug, Clone, PartialEq)]
pub struct Matrix4d {
    pub data: [[f64; 4]; 4],
}

impl Matrix4d {
    pub fn new(data: [[f64; 4]; 4]) -> Self {
        Self { data }
    }
    pub fn identity() -> Self {
        Self {
            data: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    pub fn is_identity(&self) -> bool {
        *self == Self::identity()
    }

    pub fn is_translation_only(&self) -> bool {
        // Check if only translation elements (last column, except last element) differ from identity
        let identity = Self::identity();
        for i in 0..4 {
            for j in 0..4 {
                if i == 3 && j < 3 {
                    // Skip translation elements (bottom row, first 3 columns)
                    continue;
                }
                if (self.data[i][j] - identity.data[i][j]).abs() > f64::EPSILON {
                    return false;
                }
            }
        }
        true
    }
}

impl Default for Matrix4d {
    fn default() -> Self {
        Self::identity()
    }
}

impl std::ops::Mul for Matrix4d {
    type Output = Self;

    fn mul(self, other: Self) -> Self::Output {
        let mut result = Self::default();
        for i in 0..4 {
            for j in 0..4 {
                result.data[i][j] = self.data[i][0] * other.data[0][j]
                    + self.data[i][1] * other.data[1][j]
                    + self.data[i][2] * other.data[2][j]
                    + self.data[i][3] * other.data[3][j];
            }
        }
        result
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vector3d {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vector3d {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn zero() -> Self {
        Self::new(0.0, 0.0, 0.0)
    }

    pub fn one() -> Self {
        Self::new(1.0, 1.0, 1.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quaterniond {
    pub w: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Quaterniond {
    pub fn new(w: f64, x: f64, y: f64, z: f64) -> Self {
        Self { w, x, y, z }
    }

    pub fn identity() -> Self {
        Self::new(1.0, 0.0, 0.0, 0.0)
    }

    pub fn to_matrix4(&self) -> Matrix4d {
        // Convert quaternion to 4x4 transformation matrix
        let w = self.w;
        let x = self.x;
        let y = self.y;
        let z = self.z;

        let xx = x * x;
        let yy = y * y;
        let zz = z * z;
        let xy = x * y;
        let xz = x * z;
        let xw = x * w;
        let yz = y * z;
        let yw = y * w;
        let zw = z * w;

        Matrix4d {
            data: [
                [1.0 - 2.0 * (yy + zz), 2.0 * (xy - zw), 2.0 * (xz + yw), 0.0],
                [2.0 * (xy + zw), 1.0 - 2.0 * (xx + zz), 2.0 * (yz - xw), 0.0],
                [2.0 * (xz - yw), 2.0 * (yz + xw), 1.0 - 2.0 * (xx + yy), 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }
}

impl Default for Quaterniond {
    fn default() -> Self {
        Self::identity()
    }
}

// Error type for TrsMatrix operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum TrsError {
    #[error("Matrix is not set")]
    MatrixNotSet,
    #[error("Translation is not set")]
    TranslationNotSet,
    #[error("Rotation is not set")]
    RotationNotSet,
    #[error("Scale is not set")]
    ScaleNotSet,
}

// This struct is used to store one or more of a translation, rotation, scale
// vectors or a transformation matrix.
#[derive(Debug, Clone, PartialEq)]
pub struct TrsMatrix {
    matrix: Option<Matrix4d>,
    translation: Option<Vector3d>,
    rotation: Option<Quaterniond>,
    scale: Option<Vector3d>,
}

impl Default for TrsMatrix {
    fn default() -> Self {
        Self {
            matrix: None,
            translation: None,
            rotation: None,
            scale: None,
        }
    }
}

impl TrsMatrix {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn copy(&mut self, other: &TrsMatrix) {
        *self = other.clone();
    }

    // Matrix operations
    pub fn set_matrix(&mut self, matrix: Matrix4d) -> &mut Self {
        self.matrix = Some(matrix);
        self
    }

    pub fn matrix_set(&self) -> bool {
        self.matrix.is_some()
    }

    pub fn matrix(&self) -> Result<&Matrix4d, TrsError> {
        self.matrix.as_ref().ok_or(TrsError::MatrixNotSet)
    }

    // Translation operations
    pub fn set_translation(&mut self, translation: Vector3d) -> &mut Self {
        self.translation = Some(translation);
        self
    }

    pub fn translation_set(&self) -> bool {
        self.translation.is_some()
    }

    pub fn translation(&self) -> Result<&Vector3d, TrsError> {
        self.translation.as_ref().ok_or(TrsError::TranslationNotSet)
    }

    // Rotation operations
    pub fn set_rotation(&mut self, rotation: Quaterniond) -> &mut Self {
        self.rotation = Some(rotation);
        self
    }

    pub fn rotation_set(&self) -> bool {
        self.rotation.is_some()
    }

    pub fn rotation(&self) -> Result<&Quaterniond, TrsError> {
        self.rotation.as_ref().ok_or(TrsError::RotationNotSet)
    }

    // Scale operations
    pub fn set_scale(&mut self, scale: Vector3d) -> &mut Self {
        self.scale = Some(scale);
        self
    }

    pub fn scale_set(&self) -> bool {
        self.scale.is_some()
    }

    pub fn scale(&self) -> Result<&Vector3d, TrsError> {
        self.scale.as_ref().ok_or(TrsError::ScaleNotSet)
    }

    // Returns true if the matrix is not set or if matrix is set and is equal to identity.
    pub fn is_matrix_identity(&self) -> bool {
        match &self.matrix {
            None => true,
            Some(matrix) => matrix.is_identity(),
        }
    }

    // Returns true if matrix is set and only the translation elements may differ
    // from identity. Returns false if matrix is not set.
    pub fn is_matrix_translation_only(&self) -> bool {
        match &self.matrix {
            None => false,
            Some(matrix) => matrix.is_translation_only(),
        }
    }

    // Returns transformation matrix if it has been set. Otherwise, computes
    // transformation matrix from TRS vectors and returns it.
    pub fn compute_transformation_matrix(&self) -> Matrix4d {
        if let Some(matrix) = &self.matrix {
            return matrix.clone();
        }

        // Start with identity matrix
        let mut result = Matrix4d::identity();

        // Apply scale
        if let Some(scale) = &self.scale {
            result.data[0][0] *= scale.x;
            result.data[1][1] *= scale.y;
            result.data[2][2] *= scale.z;
        }

        // Apply rotation
        if let Some(rotation) = &self.rotation {
            let _rotation_matrix = rotation.to_matrix4();
            // Matrix multiplication would go here - simplified for placeholder
            // In a real implementation, you'd multiply result * rotation_matrix
        }

        // Apply translation
        if let Some(translation) = &self.translation {
            result.data[0][3] = translation.x;
            result.data[1][3] = translation.y;
            result.data[2][3] = translation.z;
        }

        result
    }

    // Returns a boolean indicating whether any of the transforms have been set.
    // Can be used to check whether this object represents a default transform.
    pub fn transform_set(&self) -> bool {
        self.matrix.is_some() || self.translation.is_some() || self.rotation.is_some() || self.scale.is_some()
    }
}

// Stores a mapping from material index to materials variant indices. Each
// mesh instance may have multiple such mappings associated with it. See glTF
// extension KHR_materials_variants for more details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialsVariantsMapping {
    pub material: i32,
    pub variants: Vec<i32>,
}

impl MaterialsVariantsMapping {
    pub fn new(material: i32, variants: Vec<i32>) -> Self {
        Self { material, variants }
    }
}

// Describes mesh instance stored in a mesh group, including base mesh index,
// material index, and materials variants mappings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeshInstance {
    pub mesh_index: MeshIdx,
    pub material_index: i32,
    pub materials_variants_mappings: Vec<MaterialsVariantsMapping>,
}

impl MeshInstance {
    pub fn new(mesh_index: MeshIdx, material_index: i32) -> Self {
        Self {
            mesh_index,
            material_index,
            materials_variants_mappings: Vec::new(),
        }
    }

    pub fn new_with_variants(
        mesh_index: MeshIdx,
        material_index: i32,
        materials_variants_mappings: Vec<MaterialsVariantsMapping>,
    ) -> Self {
        Self {
            mesh_index,
            material_index,
            materials_variants_mappings,
        }
    }
}

// This struct is used to hold ordered mesh instances that refer to one or more
// base meshes, materials, and materials variants mappings.
#[derive(Debug, Clone, Default)]
pub struct MeshGroup {
    name: String,
    mesh_instances: Vec<MeshInstance>,
}

impl MeshGroup {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn copy(&mut self, other: &MeshGroup) {
        self.name = other.name.clone();
        self.mesh_instances = other.mesh_instances.clone();
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }

    pub fn add_mesh_instance(&mut self, instance: MeshInstance) {
        self.mesh_instances.push(instance);
    }

    pub fn set_mesh_instance(&mut self, index: usize, instance: MeshInstance) {
        if index < self.mesh_instances.len() {
            self.mesh_instances[index] = instance;
        }
    }

    pub fn get_mesh_instance(&self, index: usize) -> Option<&MeshInstance> {
        self.mesh_instances.get(index)
    }

    pub fn get_mesh_instance_mut(&mut self, index: usize) -> Option<&mut MeshInstance> {
        self.mesh_instances.get_mut(index)
    }

    pub fn num_mesh_instances(&self) -> usize {
        self.mesh_instances.len()
    }

    // Removes all mesh instances referring to base mesh at |mesh_index|.
    pub fn remove_mesh_instances(&mut self, mesh_index: MeshIdx) {
        self.mesh_instances.retain(|instance| instance.mesh_index != mesh_index);
    }
}


#[derive(Debug, Clone, Default)]
pub struct Animation;

#[derive(Debug, Clone, Default)]
pub struct Skin;

// Light type according to KHR_lights_punctual extension
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightType {
    Directional,
    Point,
    Spot,
}

impl Default for LightType {
    fn default() -> Self {
        LightType::Directional
    }
}

// Describes a light in a scene according to the KHR_lights_punctual extension.
#[derive(Debug, Clone, Default)]
pub struct Light {
    name: String,
    color: [f32; 3], // RGB color
    intensity: f64,
    light_type: LightType,
    // The range is only applicable to lights with Type::POINT or Type::SPOT.
    range: f64,
    // The cone angles are only applicable to lights with Type::SPOT.
    inner_cone_angle: f64,
    outer_cone_angle: f64,
}

impl Light {
    pub fn new() -> Self {
        Self {
            name: String::new(),
            color: [1.0, 1.0, 1.0], // Default white color
            intensity: 1.0,
            light_type: LightType::Directional,
            range: f64::INFINITY,
            inner_cone_angle: 0.0,
            outer_cone_angle: std::f64::consts::FRAC_PI_4, // π/4 radians
        }
    }

    pub fn copy(&mut self, other: &Light) {
        self.name = other.name.clone();
        self.color = other.color.clone();
        self.intensity = other.intensity;
        self.light_type = other.light_type;
        self.range = other.range;
        self.inner_cone_angle = other.inner_cone_angle;
        self.outer_cone_angle = other.outer_cone_angle;
    }

    // Name
    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    // Color
    pub fn set_color(&mut self, color: [f32; 3]) {
        self.color = color;
    }

    pub fn get_color(&self) -> &[f32; 3] {
        &self.color
    }

    // Intensity
    pub fn set_intensity(&mut self, intensity: f64) {
        self.intensity = intensity;
    }

    pub fn get_intensity(&self) -> f64 {
        self.intensity
    }

    // Type
    pub fn set_type(&mut self, light_type: LightType) {
        self.light_type = light_type;
    }

    pub fn get_type(&self) -> LightType {
        self.light_type
    }

    // Range
    pub fn set_range(&mut self, range: f64) {
        self.range = range;
    }

    pub fn get_range(&self) -> f64 {
        self.range
    }

    // Inner cone angle
    pub fn set_inner_cone_angle(&mut self, angle: f64) {
        self.inner_cone_angle = angle;
    }

    pub fn get_inner_cone_angle(&self) -> f64 {
        self.inner_cone_angle
    }

    // Outer cone angle
    pub fn set_outer_cone_angle(&mut self, angle: f64) {
        self.outer_cone_angle = angle;
    }

    pub fn get_outer_cone_angle(&self) -> f64 {
        self.outer_cone_angle
    }
}

// Instance data for mesh group instancing according to EXT_mesh_gpu_instancing
#[derive(Debug, Clone, Default)]
pub struct Instance {
    // Translation, rotation, and scale vectors.
    pub trs: TrsMatrix,
    // TODO: Support custom instance attributes, e.g., _ID, _COLOR, etc.
}

impl Instance {
    pub fn new() -> Self {
        Self {
            trs: TrsMatrix::new(),
        }
    }

    pub fn new_with_trs(trs: TrsMatrix) -> Self {
        Self { trs }
    }
}

// Error type for InstanceArray operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum InstanceArrayError {
    #[error("Instance must have no matrix set - only individual TRS vectors are allowed")]
    MatrixNotAllowed,
    #[error("Instance index {0} is out of range (max: {1})")]
    IndexOutOfRange(usize, usize),
}

// Array of instances for mesh group instancing
#[derive(Debug, Clone, Default)]
pub struct InstanceArray {
    instances: Vec<Instance>,
}

impl InstanceArray {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn copy(&mut self, other: &InstanceArray) {
        self.instances.clear();
        self.instances.reserve(other.instances.len());
        for instance in &other.instances {
            let mut new_instance = Instance::new();
            new_instance.trs.copy(&instance.trs);
            self.instances.push(new_instance);
        }
    }

    // Adds one |instance| into this mesh group instance array where the
    // |instance.trs| may have optional translation, rotation, and scale set.
    pub fn add_instance(&mut self, instance: Instance) -> Result<(), InstanceArrayError> {
        // Check that the |instance.trs| does not have the transformation matrix set,
        // because the EXT_mesh_gpu_instancing glTF extension dictates that only the
        // individual TRS vectors are stored.
        if instance.trs.matrix_set() {
            return Err(InstanceArrayError::MatrixNotAllowed);
        }

        // Move the |instance| to the end of the instances vector.
        self.instances.push(instance);
        Ok(())
    }

    // Returns the count of instances in this mesh group instance array.
    pub fn num_instances(&self) -> usize {
        self.instances.len()
    }

    // Returns an instance from this mesh group instance array.
    pub fn get_instance(&self, index: usize) -> Result<&Instance, InstanceArrayError> {
        self.instances.get(index).ok_or_else(|| {
            InstanceArrayError::IndexOutOfRange(index, self.instances.len())
        })
    }

    // Returns a mutable reference to an instance from this mesh group instance array.
    pub fn get_instance_mut(&mut self, index: usize) -> Result<&mut Instance, InstanceArrayError> {
        let len = self.instances.len();
        self.instances.get_mut(index).ok_or_else(|| {
            InstanceArrayError::IndexOutOfRange(index, len)
        })
    }

    // Returns all instances as a slice.
    pub fn instances(&self) -> &[Instance] {
        &self.instances
    }

    // Returns all instances as a mutable slice.
    pub fn instances_mut(&mut self) -> &mut [Instance] {
        &mut self.instances
    }

    // Removes all instances.
    pub fn clear(&mut self) {
        self.instances.clear();
    }

    // Removes an instance at the specified index.
    pub fn remove_instance(&mut self, index: usize) -> Result<Instance, InstanceArrayError> {
        if index >= self.instances.len() {
            return Err(InstanceArrayError::IndexOutOfRange(index, self.instances.len()));
        }
        Ok(self.instances.remove(index))
    }

    // Reserves capacity for at least |additional| more instances.
    pub fn reserve(&mut self, additional: usize) {
        self.instances.reserve(additional);
    }
}


#[derive(Debug, Clone, thiserror::Error)]
pub enum Err {
    #[error("Mesh index out of range: the index {0} is greater than the number of meshes {1}")]
    MeshIndexOutOfRange(usize, usize),
    #[error("Mesh group index out of range: the index {0} is greater than the number of mesh groups {1}")]
    MeshGroupIndexOutOfRange(usize, usize),
    #[error("Scene node index out of range: the index {0} is greater than the number of scene nodes {1}")]
    SceneNodeIndexOutOfRange(usize, usize),
    #[error("Skin index out of range: the index {0} is greater than the number of skins {1}")]
    SkinIndexOutOfRange(usize, usize),
    #[error("Light index out of range: the index {0} is greater than the number of lights {1}")]
    LightIndexOutOfRange(usize, usize),
    #[error("Instance array index out of range: the index {0} is greater than the number of instance arrays {1}")]
    InstanceArrayIndexOutOfRange(usize, usize),
    #[error("Material index out of range: the index {0} is greater than the number of materials {1}")]
    MaterialIndexOutOfRange(usize, usize),
    #[error("Failed to remove material at index {0}: the material is used in the scene")]
    MaterialUsedInScene(usize),
    #[error("Failed to remove mesh group at index {0}: the mesh group is used in the scene")]
    MeshGroupUsedInScene(usize),
}

// Class used to hold all of the geometry to create a scene. A scene is
// comprised of one or more meshes, one or more scene nodes, one or more
// mesh groups, and a material library. The meshes are defined in their
// local space. A mesh group is a list of meshes. The scene nodes create
// a scene hierarchy to transform meshes in their local space into scene space.
// The material library contains all of the materials and textures used by the
// meshes in this scene.
#[derive(Clone, Debug)]
pub struct Scene {
    meshes: Vec<Mesh>,
    mesh_groups: Vec<MeshGroup>,
    nodes: Vec<SceneNode>,
    root_node_indices: Vec<SceneNodeIdx>,
    #[allow(unused)]
    skins: Vec<Skin>,

    // The lights will be written to the output scene but not used for internal
    // rendering in Draco, e.g, while computing distortion metric.
    #[allow(unused)]
    lights: Vec<Light>,

    // The mesh group instance array information will be written to the output
    // scene but not processed by Draco simplifier modules.
    #[allow(unused)]
    instance_arrays: Vec<InstanceArray>,

    // Materials used by this scene.
    material_library: MaterialLibrary,

    // Texture library for storing non-material textures used by this scene, e.g.,
    // textures containing mesh feature IDs of EXT_mesh_features glTF extension.
    // Note that scene meshes contain pointers to non-material textures. It is
    // responsibility of class user to update these pointers when updating the
    // textures. See Scene::Copy() for example.
    #[allow(unused)]
    non_material_texture_library: TextureLibrary,

    // Structural metadata defined by the EXT_structural_metadata glTF extension.
    #[allow(unused)]
    structural_metadata: StructuralMetadata,

    // General metadata associated with the scene (not related to the
    // EXT_structural_metadata extension).
    metadata: Metadata,

    #[allow(unused)]
    animations: Vec<Animation>,
}



impl Scene {
    pub(crate) fn new() -> Self {
        Self {
            meshes: Vec::new(),
            mesh_groups: Vec::new(),
            nodes: Vec::new(),
            root_node_indices: Vec::new(),
            skins: Vec::new(),
            lights: Vec::new(),
            instance_arrays: Vec::new(),
            material_library: MaterialLibrary::new(),
            non_material_texture_library: TextureLibrary::new(),
            structural_metadata: StructuralMetadata::default(),
            metadata: Metadata::new(),
            animations: Vec::new(),
        }
    }
    pub(crate) fn add_mesh(&mut self, mesh: Mesh) -> MeshIdx {
        self.meshes.push(mesh);
        self.meshes.len() - 1
    }

    pub(crate) fn get_mesh(&self, idx: MeshIdx) -> Option<&Mesh> { 
        self.meshes.get(idx)
    }

    pub(crate) fn add_mesh_group(&mut self) -> MeshGroupIdx {
        self.mesh_groups.push(MeshGroup::new());
        self.mesh_groups.len() - 1
    }

    pub(crate) fn get_mesh_group(&self, index: MeshGroupIdx) -> Option<&MeshGroup> {
        self.mesh_groups.get(index)
    }

    pub(crate) fn get_mesh_group_mut(&mut self, index: MeshGroupIdx) -> Option<&mut MeshGroup> {
        self.mesh_groups.get_mut(index)
    }

    pub(crate) fn add_node(&mut self, node: SceneNode) -> SceneNodeIdx {
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    pub(crate) fn num_nodes(&self)-> usize { self.nodes.len() }

    pub(crate) fn get_node(&self, index: SceneNodeIdx) -> Option<&SceneNode> {
        self.nodes.get(index)
    }

    pub(crate) fn get_node_mut(&mut self, index: SceneNodeIdx) -> Option<&mut SceneNode> {
        self.nodes.get_mut(index)
    }

    pub(crate) fn add_root_node_index(&mut self, index: SceneNodeIdx) {
        self.root_node_indices.push(index);
    }

    pub(crate) fn material_library(&self) -> &MaterialLibrary {
        &self.material_library
    }

    pub(crate) fn material_library_mut(&mut self) -> &mut MaterialLibrary {
        &mut self.material_library
    }

    pub(crate) fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    pub(crate) fn metadata_mut(&mut self) -> &mut Metadata {
        &mut self.metadata
    }
}


// This struct is used to create a scene hierarchy from meshes in their local
// space transformed into scene space.
#[derive(Debug, Clone, Default)]
pub struct SceneNode {
    name: String,
    trs_matrix: TrsMatrix,
    pub mesh_group_index: Option<MeshGroupIdx>,
    skin_index: Option<SkinIdx>,
    parents: Vec<SceneNodeIdx>,
    children: Vec<SceneNodeIdx>,
    light_index: Option<LightIdx>,
    instance_array_index: Option<InstanceArrayIdx>,
}

impl SceneNode {
    pub fn new() -> Self {
        Self::default()
    }

    // Sets a name.
    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }

    // Returns the name.
    pub fn get_name(&self) -> &str {
        &self.name
    }

    // Set transformation from mesh local space to scene space.
    pub fn set_trs_matrix(&mut self, trs_matrix: TrsMatrix) {
        self.trs_matrix.copy(&trs_matrix);
    }

    pub fn get_trs_matrix(&self) -> &TrsMatrix {
        &self.trs_matrix
    }

    // Set the index to the mesh group in the scene.
    pub fn set_mesh_group_index(&mut self, index: Option<MeshGroupIdx>) {
        self.mesh_group_index = index;
    }

    pub fn get_mesh_group_index(&self) -> Option<MeshGroupIdx> {
        self.mesh_group_index
    }

    // Set the index to the skin in the scene.
    pub fn set_skin_index(&mut self, index: Option<SkinIdx>) {
        self.skin_index = index;
    }

    pub fn get_skin_index(&self) -> Option<SkinIdx> {
        self.skin_index
    }

    // Set the index to the light in the scene.
    pub fn set_light_index(&mut self, index: Option<LightIdx>) {
        self.light_index = index;
    }

    pub fn get_light_index(&self) -> Option<LightIdx> {
        self.light_index
    }

    // Set the index to the mesh group instance array in the scene. Note that
    // according to EXT_mesh_gpu_instancing glTF extension there is no defined
    // behavior for a node with instance array and without a mesh group.
    pub fn set_instance_array_index(&mut self, index: Option<InstanceArrayIdx>) {
        self.instance_array_index = index;
    }

    pub fn get_instance_array_index(&self) -> Option<InstanceArrayIdx> {
        self.instance_array_index
    }

    // Functions to set and get zero or more parent nodes of this node.
    pub fn parent(&self, index: usize) -> Option<SceneNodeIdx> {
        self.parents.get(index).copied()
    }

    pub fn parents(&self) -> &[SceneNodeIdx] {
        &self.parents
    }

    pub fn add_parent_index(&mut self, index: SceneNodeIdx) {
        self.parents.push(index);
    }

    pub fn num_parents(&self) -> usize {
        self.parents.len()
    }

    pub fn remove_all_parents(&mut self) {
        self.parents.clear();
    }

    // Functions to set and get zero or more child nodes of this node.
    pub fn child(&self, index: usize) -> Option<SceneNodeIdx> {
        self.children.get(index).copied()
    }

    pub fn children(&self) -> &[SceneNodeIdx] {
        &self.children
    }

    pub fn add_child_index(&mut self, index: SceneNodeIdx) {
        self.children.push(index);
    }

    pub fn num_children(&self) -> usize {
        self.children.len()
    }

    pub fn remove_all_children(&mut self) {
        self.children.clear();
    }
}




