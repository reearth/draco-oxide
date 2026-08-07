//! Buffer builder with alignment support.

/// Builder for constructing a GLB binary buffer with proper alignment.
#[derive(Debug, Default)]
pub struct BufferBuilder {
    data: Vec<u8>,
}

impl BufferBuilder {
    /// Creates an empty buffer builder.
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }

    /// The current buffer length in bytes, including padding.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Append data with specified alignment. Returns (byte_offset, byte_length).
    pub fn append(&mut self, data: &[u8], alignment: usize) -> (usize, usize) {
        let padding = (alignment - (self.data.len() % alignment)) % alignment;
        self.data.extend(std::iter::repeat_n(0u8, padding));

        let offset = self.data.len();
        let length = data.len();
        self.data.extend_from_slice(data);

        (offset, length)
    }

    /// Consumes the builder and returns the assembled buffer bytes.
    pub fn finish(self) -> Vec<u8> {
        self.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_append() {
        let mut builder = BufferBuilder::new();

        let (offset1, len1) = builder.append(b"hello", 4);
        assert_eq!(offset1, 0);
        assert_eq!(len1, 5);

        let (offset2, len2) = builder.append(b"world", 4);
        assert_eq!(offset2, 8); // 5 + 3 padding
        assert_eq!(len2, 5);
    }

    #[test]
    fn test_already_aligned() {
        let mut builder = BufferBuilder::new();

        let (offset1, _) = builder.append(b"1234", 4);
        assert_eq!(offset1, 0);

        let (offset2, _) = builder.append(b"5678", 4);
        assert_eq!(offset2, 4);
    }
}
