use std::collections::HashMap;

use crate::{core::{texture::{self, TextureLibrary, TextureMap}}, prelude::NdVector};

#[derive(Clone, Debug)]
pub(crate) struct MaterialLibrary {
    materials: Vec<Material>,
    texture_library: TextureLibrary,
}

impl MaterialLibrary {
    pub fn new() -> Self {
        Self {
            materials: Vec::new(),
            texture_library: TextureLibrary::new(),
        }
    }

    pub fn num_materials(&self) -> usize {
        self.materials.len()
    }

    pub fn get_material(&self, index: usize) -> Option<&Material> {
        self.materials.get(index)
    }

    pub fn mutable_material(&mut self, index: usize) -> Option<&mut Material> {
        if index >= self.materials.len() {
            self.materials.resize_with(index + 1, || Material::new());
        }
        self.materials.get_mut(index)
    }

    pub fn get_texture_library(&self) -> &TextureLibrary {
        &self.texture_library
    }

    pub fn get_texture_library_mut(&mut self) -> &mut TextureLibrary {
        &mut self.texture_library
    }

    /// Adds a material to the library and returns its index.
    pub fn add_material(&mut self, material: Material) -> usize {
        self.materials.push(material);
        self.materials.len() - 1
    }
}


#[derive(Clone, Debug)]
pub(crate) struct Material {
    name: String,
    color_factor: NdVector<4, f32>,
    metallic_factor: f32,
    roughness_factor: f32,
    emissive_factor: NdVector<3, f32>,
    double_sided: bool,
    transparency_mode: TransparencyMode,
    alpha_cutoff: f32,
    #[allow(unused)]
    normal_texture_scale: f32,
    unlit: bool,
    has_sheen: bool,
    #[allow(unused)]
    sheen_color_factor: NdVector<3, f32>,
    #[allow(unused)]
    sheen_roughness_factor: f32,
    has_transmission: bool,
    #[allow(unused)]
    transmission_factor: f32,
    has_clearcoat: bool,
    #[allow(unused)]
    clearcoat_factor: f32,
    #[allow(unused)]
    clearcoat_roughness_factor: f32,
    has_volume: bool,
    #[allow(unused)]
    thickness_factor: f32,
    #[allow(unused)]
    attenuation_distance: f32,
    #[allow(unused)]
    attenuation_color: NdVector<3, f32>,
    has_ior: bool,
    #[allow(unused)]
    ior: f32,
    has_specular: bool,
    #[allow(unused)]
    specular_factor: f32,
    #[allow(unused)]
    specular_color_factor: NdVector<3, f32>,
    texture_maps: Vec<Box<TextureMap>>,
    texture_map_type_to_index_map: HashMap<texture::Type, usize>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransparencyMode {
    Opaque = 0,
    Mask,
    Blend,
}

impl Material {
    pub fn new() -> Self {
        Self {
            name: String::new(),
            color_factor: NdVector::from([1.0, 1.0, 1.0, 1.0]),
            metallic_factor: 1.0,
            roughness_factor: 1.0,
            emissive_factor: NdVector::from([0.0, 0.0, 0.0]),
            double_sided: false,
            transparency_mode: TransparencyMode::Opaque,
            alpha_cutoff: 0.5,
            normal_texture_scale: 1.0,
            unlit: false,
            has_sheen: false,
            sheen_color_factor: NdVector::from([0.0, 0.0, 0.0]),
            sheen_roughness_factor: 0.0,
            has_transmission: false,
            transmission_factor: 0.0,
            has_clearcoat: false,
            clearcoat_factor: 0.0,
            clearcoat_roughness_factor: 0.0,
            has_volume: false,
            thickness_factor: 0.0,
            attenuation_distance: f32::INFINITY,
            attenuation_color: NdVector::from([1.0, 1.0, 1.0]),
            has_ior: false,
            ior: 1.5,
            has_specular: false,
            specular_factor: 1.0,
            specular_color_factor: NdVector::from([1.0, 1.0, 1.0]),
            texture_maps: Vec::new(),
            texture_map_type_to_index_map: HashMap::new(),
        }
    }

    pub fn is_unlit_fallback_required(&self) -> bool {
        self.unlit
    }

    pub fn get_texture_map_by_index(&self, index: usize) -> Option<&TextureMap> {
        self.texture_maps.get(index).map(|tm| tm.as_ref())
    }

    pub fn get_texture_map_by_type(&self, texture_type: texture::Type) -> Option<&TextureMap> {
        self.texture_map_type_to_index_map.get(&texture_type)
            .and_then(|&idx| self.get_texture_map_by_index(idx))
    }

    // Getter methods for the encode function
    pub fn get_color_factor(&self) -> &NdVector<4, f32> {
        &self.color_factor
    }

    pub fn get_metallic_factor(&self) -> f32 {
        self.metallic_factor
    }

    pub fn get_roughness_factor(&self) -> f32 {
        self.roughness_factor
    }

    pub fn get_emissive_factor(&self) -> &NdVector<3, f32> {
        &self.emissive_factor
    }

    // Setter methods
    pub fn set_metallic_factor(&mut self, value: f32) {
        self.metallic_factor = value;
    }

    pub fn set_color_factor(&mut self, color: NdVector<4, f32>) {
        self.color_factor = color;
    }

    pub fn set_roughness_factor(&mut self, value: f32) {
        self.roughness_factor = value;
    }

    pub fn get_transparency_mode(&self) -> TransparencyMode {
        self.transparency_mode
    }

    pub fn get_alpha_cutoff(&self) -> f32 {
        self.alpha_cutoff
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }

    pub fn is_double_sided(&self) -> bool {
        self.double_sided
    }

    pub fn check_any_pbr_extensions(&self) -> bool {
        self.has_sheen || self.has_transmission || self.has_clearcoat ||
        self.has_volume || self.has_ior || self.has_specular 
        || self.unlit
    }

    pub fn has_sheen(&self) -> bool {
        self.has_sheen
    }
    pub fn has_transmission(&self) -> bool {
        self.has_transmission
    }
    pub fn has_clearcoat(&self) -> bool {
        self.has_clearcoat
    }
    pub fn has_volume(&self) -> bool {
        self.has_volume
    }
    pub fn has_ior(&self) -> bool {
        self.has_ior
    }
    pub fn has_specular(&self) -> bool {
        self.has_specular
    }

    /// Adds a texture map to the material and returns its index.
    /// If a texture map with the same type already exists, it will be replaced.
    pub fn set_texture_map(&mut self, texture_type: texture::Type, texture_map: TextureMap) {
        // Check if we already have a texture map of this type
        if let Some(&existing_index) = self.texture_map_type_to_index_map.get(&texture_type) {
            // Replace existing texture map
            if existing_index < self.texture_maps.len() {
                self.texture_maps[existing_index] = Box::new(texture_map);
            }
        } else {
            // Add new texture map
            let index = self.texture_maps.len();
            self.texture_maps.push(Box::new(texture_map));
            self.texture_map_type_to_index_map.insert(texture_type, index);
        }
    }
}
