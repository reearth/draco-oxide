use std::usize;
use std::collections::HashSet;

use crate::core::material::MaterialLibrary;

#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Texture {
    img: Image
}

impl Texture {
    pub(crate) fn new() -> Self {
        Self {
            img: Image::new(),
        }
    }
    pub(crate) fn set_source_image(&mut self, img: Image) { self.img = img }
    pub(crate) fn get_source_image(&self) -> &Image {& self.img }
}


#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Image {
    filename: String,
    mime_type: String,
    encoded_data: Vec<u8>,
}

impl Image {
    pub(crate) fn new() -> Self { Self {
        filename: String::new(),
        mime_type: String::new(),
        encoded_data: Vec::new(),
    } }

    
    // Sets the name of the source image file.
    pub(crate) fn set_filename(&mut self, filename: String) { self.filename = filename; }
    
    
    pub(crate) fn get_filename(&self) -> &String { &self.filename }
    
    pub(crate) fn set_mime_type(&mut self, mime_type: String) { self.mime_type = mime_type; }
    pub(crate) fn get_mime_type(&self) -> &String { &self.mime_type }
    
    pub(crate) fn get_encoded_data_mut(&mut self) -> &mut Vec<u8> { &mut self.encoded_data }
    pub(crate) fn get_encoded_data(&self) -> &Vec<u8> { &self.encoded_data }
}


#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct TextureLibrary {
    textures: Vec<Texture>,
}

impl TextureLibrary {
    pub fn new() -> Self {
        Self {
            textures: Vec::new(),
        }
    }

    // Pushes a new texture into the library. Returns an index of the newly inserted texture.
    pub fn push(&mut self, texture: Texture) -> usize {
        self.textures.push(texture);
        self.textures.len() - 1
    }

    #[allow(unused)]
    pub fn num_textures(&self) -> usize {
        self.textures.len()
    }

    #[allow(unused)]
    pub fn get_mut(&mut self, index: usize) -> Option<&mut Texture> {
        self.textures.get_mut(index)
    }

    pub fn get(&self, index: usize) -> Option<&Texture> {
        self.textures.get(index)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) enum Type {
    Generic = 0,
    Color = 1,
    Opacity = 2,
    Metallic = 3,
    Roughness = 4,
    MetallicRoughness = 5,
    NormalObjectSpace = 6,
    NormalTangentSpace = 7,
    AmbientOcclusion = 8,
    Emissive = 9,
    SheenColor = 10,
    SheenRoughness = 11,
    Transmission = 12,
    Clearcoat = 13,
    ClearcoatRoughness = 14,
    ClearcoatNormal = 15,
    Thickness = 16,
    Specular = 17,
    SpecularColor = 18,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum AxisWrappingMode {
    // Out of bounds access along a texture axis should be clamped to the
    // nearest edge (default).
    ClampToEdge = 0,
    // Texture is repeated along a texture axis in a mirrored pattern.
    MirroredRepeat,
    // Texture is repeated along a texture axis (tiled textures).
    Repeat
}

impl AxisWrappingMode {
    #[allow(unused)]
    pub fn as_i32(&self) -> i32 {
        match self {
            AxisWrappingMode::ClampToEdge => 0,
            AxisWrappingMode::MirroredRepeat => 1,
            AxisWrappingMode::Repeat => 2,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate)  struct WrappingMode {
    s: AxisWrappingMode,
    t: AxisWrappingMode,
}

impl WrappingMode {
    pub fn new(s: AxisWrappingMode, t: AxisWrappingMode) -> Self {Self{s, t}}
    pub fn new_with_single_mode(mode: AxisWrappingMode) -> Self {Self{s: mode, t: mode}}
    pub fn s(&self) -> AxisWrappingMode {self.s}
    pub fn t(&self) -> AxisWrappingMode {self.t}
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum FilterType {
    Unspecified = 0,
    Nearest,
    Linear,
    NearestMipmapNearest,
    LinearMipmapNearest,
    NearestMipmapLinear,
    LinearMipmapLinear
}

impl FilterType {
    #[allow(unused)]
    pub fn as_i32(&self) -> i32 {
        match self {
            FilterType::Unspecified => 0,
            FilterType::Nearest => 1,
            FilterType::Linear => 2,
            FilterType::NearestMipmapNearest => 3,
            FilterType::LinearMipmapNearest => 4,
            FilterType::NearestMipmapLinear => 5,
            FilterType::LinearMipmapLinear => 6,
        }
    }
}


#[derive(Clone, PartialEq, Debug)]
pub(crate) struct TextureMap {
    ty: Type,
    wrapping_mode: WrappingMode,
    tex_coord_index: isize,
    min_filter: FilterType,
    mag_filter: FilterType,
    texture: Texture,
    transform: TextureTransform,
}

impl TextureMap {
    pub(crate) fn new() -> Self {
        Self {
            ty: Type::Generic,
            wrapping_mode: WrappingMode::new_with_single_mode(AxisWrappingMode::ClampToEdge),
            tex_coord_index: isize::MAX,
            min_filter: FilterType::Unspecified,
            mag_filter: FilterType::Unspecified,
            texture: Texture::new(),
            transform: TextureTransform::default(),
        }
    }

    pub(crate) fn set_properties(
        &mut self, ty: Type, 
        wrapping_mode: Option<WrappingMode>, 
        tex_coord_index: Option<isize>, 
        min_filter: Option<FilterType>, 
        mag_filter: Option<FilterType>
    ) {
        self.ty = ty;
        self.wrapping_mode = wrapping_mode.unwrap_or(WrappingMode::new_with_single_mode(AxisWrappingMode::ClampToEdge));
        self.tex_coord_index = tex_coord_index.unwrap_or(0);
        self.min_filter = min_filter.unwrap_or(FilterType::Unspecified);
        self.mag_filter = mag_filter.unwrap_or(FilterType::Unspecified);
    }

    pub(crate) fn set_texture(&mut self, texture: Texture) {
        self.texture = texture;
    }

    pub(crate) fn get_transform(&self) -> &TextureTransform {
        &self.transform
    }

    pub(crate) fn get_texture(&self) -> &Texture { &self.texture }
    pub(crate) fn get_wrapping_mode(&self) -> WrappingMode { self.wrapping_mode }
    pub(crate) fn tex_coord_index(&self) -> isize { self.tex_coord_index }
    pub(crate) fn min_filter(&self) -> FilterType { self.min_filter }
    pub(crate) fn mag_filter(&self) -> FilterType { self.mag_filter }
}



#[derive(Clone, PartialEq, Debug)]
pub(crate) struct TextureTransform {
    offset: [f64; 2],
    rotation: f64,
    scale: [f64; 2],
    tex_coord: i32,
}

impl Default for TextureTransform {
    fn default() -> Self {
        Self {
            offset: TextureTransform::get_default_offset(),
            rotation: TextureTransform::get_default_rotation(),
            scale: TextureTransform::get_default_scale(),
            tex_coord: TextureTransform::get_default_tex_coord(),
        }
    }
}

impl TextureTransform {
    // Returns true if |tt| contains all default values.
    pub fn is_default(tt: &TextureTransform) -> bool {
        tt == &TextureTransform::default()
    }

    pub fn is_offset_set(&self) -> bool {
        self.offset != Self::get_default_offset()
    }

    pub fn is_rotation_set(&self) -> bool {
        self.rotation != Self::get_default_rotation()
    }

    pub fn is_scale_set(&self) -> bool {
        self.scale != Self::get_default_scale()
    }

    pub fn is_tex_coord_set(&self) -> bool {
        self.tex_coord != Self::get_default_tex_coord()
    }

    pub fn offset(&self) -> &[f64; 2] {
        &self.offset
    }

    pub fn scale(&self) -> &[f64; 2] {
        &self.scale
    }

    pub fn rotation(&self) -> f64 {
        self.rotation
    }

    pub fn tex_coord(&self) -> i32 {
        self.tex_coord
    }

    fn get_default_offset() -> [f64; 2] { return [0.0, 0.0]; }
    fn get_default_rotation() -> f64 { return 0.0; }
    fn get_default_scale() -> [f64; 2] { return [0.0, 0.0]; }
    fn get_default_tex_coord() -> i32 { return -1; }
}


// Helper struct implementing various utilities operating on Texture.
pub(crate) struct TextureUtils;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImageFormat {
    None,
    Png,
    Jpeg,
    Basis,
    Webp,
}

impl TextureUtils {
    /// Returns the image stem (file basename without extension) based on the source image filename,
    /// or an empty string when source image is not set.
    pub fn get_target_stem(texture: &Texture) -> String {
        let filename = texture.img.get_filename();
        if !filename.is_empty() {
            let filename = std::path::Path::new(filename);
            if let Some(stem) = filename.file_stem() {
                return stem.to_string_lossy().to_string();
            }
        }
        String::new()
    }

    /// Returns the image stem (file basename without extension) based on the source image filename,
    /// or a name generated from index and suffix like "Texture5_BaseColor" when source image is not set.
    pub fn get_or_generate_target_stem(texture: &Texture, index: usize, suffix: &str) -> String {
        let name = Self::get_target_stem(texture);
        if !name.is_empty() {
            name
        } else {
            format!("Texture{}{}", index, suffix)
        }
    }

    /// Returns the texture format based on compression settings, the source image mime type or the source image filename.
    pub fn get_target_format(texture: &Texture) -> ImageFormat {
        Self::get_source_format(texture)
    }

    /// Returns the image file extension based on compression settings, the source image mime type or the source image filename.
    pub fn get_target_extension(texture: &Texture) -> String {
        Self::get_extension(Self::get_target_format(texture))
    }

    /// Returns mime type string based on compression settings, source image mime type or the source image filename.
    pub fn get_target_mime_type(texture: &Texture) -> String {
        let format = Self::get_target_format(texture);
        if format == ImageFormat::None {
            let mime_type = texture.img.get_mime_type();
            if !mime_type.is_empty() {
                return mime_type.clone();
            } else {
                let filename = texture.img.get_filename();
                if !filename.is_empty() {
                    let ext = Self::lowercase_file_extension(filename);
                    if !ext.is_empty() {
                        return format!("image/{}", ext);
                    }
                }
            }
        }
        Self::get_mime_type(format)
    }

    /// Returns mime type string for a given image format.
    pub fn get_mime_type(image_format: ImageFormat) -> String {
        match image_format {
            ImageFormat::Png => "image/png".to_string(),
            ImageFormat::Jpeg => "image/jpeg".to_string(),
            ImageFormat::Basis => "image/ktx2".to_string(),
            ImageFormat::Webp => "image/webp".to_string(),
            ImageFormat::None => String::new(),
        }
    }

    /// Returns the texture format based on source image mime type or the source image filename.
    pub fn get_source_format(texture: &Texture) -> ImageFormat {
        let mut extension = Self::lowercase_mime_type_extension(texture.img.get_mime_type());
        if extension.is_empty() && !texture.img.get_filename().is_empty() {
            extension = Self::lowercase_file_extension(texture.img.get_filename());
        }
        if extension.is_empty() {
            extension = "png".to_string();
        }
        Self::get_format(&extension)
    }

    /// Returns image format corresponding to a given image file extension. NONE is returned when extension is empty or unknown.
    pub fn get_format(extension: &str) -> ImageFormat {
        match extension {
            "png" => ImageFormat::Png,
            "jpg" | "jpeg" => ImageFormat::Jpeg,
            "basis" | "ktx2" => ImageFormat::Basis,
            "webp" => ImageFormat::Webp,
            _ => ImageFormat::None,
        }
    }

    /// Returns image file extension corresponding to a given image format. Empty extension is returned when the format is NONE.
    pub fn get_extension(format: ImageFormat) -> String {
        match format {
            ImageFormat::Png => "png".to_string(),
            ImageFormat::Jpeg => "jpg".to_string(),
            ImageFormat::Basis => "ktx2".to_string(),
            ImageFormat::Webp => "webp".to_string(),
            ImageFormat::None => String::new(),
        }
    }

    /// Returns the number of channels required for encoding a texture from a given material library,
    /// taking into account texture opacity and assuming that occlusion and metallic-roughness texture maps may share a texture.
    pub fn compute_required_num_channels(
        texture: &Texture,
        material_library: &MaterialLibrary,
    ) -> usize {
        let mr_textures = Self::find_textures(Type::MetallicRoughness, material_library);
        if !mr_textures.iter().any(|t| std::ptr::eq(*t, texture)) {
            // Occlusion-only texture.
            1
        } else {
            // Occlusion-metallic-roughness texture.
            3
        }
    }

    /// Find textures with no duplicates for a given texture type in the material library.
    pub fn find_textures<'a>(
        texture_type: Type,
        material_library: &'a MaterialLibrary,
    ) -> Vec<&'a Texture> {
        let mut textures = HashSet::new();
        for i in 0..material_library.num_materials() {
            if let Some(Some(texture_map)) = material_library.get_material(i).map(|m| m.get_texture_map_by_type(texture_type)) {
                let texture_ref = texture_map.get_texture();
                textures.insert(texture_ref as *const Texture);
            }
        }
        // Convert back to references
        textures
            .into_iter()
            .filter_map(|ptr| unsafe { ptr.as_ref() })
            .collect()
    }

    // Helper: get lowercase file extension from filename
    pub(crate) fn lowercase_file_extension(filename: &str) -> String {
        std::path::Path::new(filename)
            .extension()
            .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default()
    }

    // Helper: get lowercase extension from mime type string
    pub(crate) fn lowercase_mime_type_extension(mime_type: &str) -> String {
        // e.g. "image/png" -> "png"
        mime_type.split('/').nth(1).map(|s| s.to_ascii_lowercase()).unwrap_or_default()
    }
}