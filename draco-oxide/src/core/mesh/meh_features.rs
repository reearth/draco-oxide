use crate::core::texture::TextureMap;

#[derive(Clone, Debug)]
pub(crate) struct MeshFeatures {
    label: String,
    feature_count: i32,
    null_feature_id: i32,
    attribute_index: i32,
    texture_map: TextureMap,
    texture_channels: Vec<i32>,
    property_table_index: i32,
}

impl MeshFeatures {
    pub fn new() -> Self {
        Self {
            label: String::new(),
            feature_count: 0,
            null_feature_id: -1,
            attribute_index: -1,
            texture_map: TextureMap::new(),
            texture_channels: Vec::new(),
            property_table_index: -1,
        }
    }
    
    pub fn set_label(&mut self, label: &str) {
        self.label = label.to_owned();
    }

    pub fn get_label(&self) -> &str {
        &self.label
    }

    pub fn set_feature_count(&mut self, count: i32) {
        self.feature_count = count;
    }

    pub fn get_feature_count(&self) -> i32 {
        self.feature_count
    }

    pub fn set_null_feature_id(&mut self, id: i32) {
        self.null_feature_id = id;
    }

    pub fn get_null_feature_id(&self) -> i32 {
        self.null_feature_id
    }

    pub fn set_attribute_index(&mut self, index: i32) {
        self.attribute_index = index;
    }

    pub fn get_attribute_index(&self) -> i32 {
        self.attribute_index
    }

    pub fn set_texture_map(&mut self, map: &TextureMap) {
        self.texture_map = map.clone();
    }

    pub fn get_texture_map(&self) -> &TextureMap {
        &self.texture_map
    }

    pub fn set_texture_channels(&mut self, channels: &[i32]) {
        self.texture_channels = channels.to_vec();
    }

    pub fn set_property_table_index(&mut self, index: i32) {
        self.property_table_index = index;
    }

    pub fn get_property_table_index(&self) -> i32 {
        self.property_table_index
    }
}