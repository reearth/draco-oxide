#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Metadata {
    entries: std::collections::HashMap<String, String>,
}

impl Metadata {
    pub fn new() -> Self {
        Self {
            entries: std::collections::HashMap::new(),
        }
    }
    
    pub fn add_entry(&mut self, key: String, value: String) {
        self.entries.insert(key, value);
    }
    
    pub fn get_entry(&self, key: &str) -> Option<&String> {
        self.entries.get(key)
    }
}