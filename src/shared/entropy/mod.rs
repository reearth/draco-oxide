
pub trait Symbol {
    fn cardinality() -> usize;
    fn size(&self) -> usize;
    fn get_id(&self) -> usize;
    fn from_id(id: usize) -> Self;
}