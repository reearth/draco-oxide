use super::attribute::Attribute;

pub struct Mesh {
    faces: Vec<[usize; 3]>,
	attributes: Vec<Attribute>,
}

impl Mesh {
    pub fn get_faces(&self) -> &[[usize; 3]] {
        &self.faces
    }

    pub fn get_attributes(&self) -> &[Attribute] {
        &self.attributes
    }
}