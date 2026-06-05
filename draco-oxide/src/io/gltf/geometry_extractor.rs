//! Geometry extraction from glTF accessors.

use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Accessor {0} not found")]
    AccessorNotFound(u64),
    #[error("BufferView {0} not found")]
    BufferViewNotFound(u64),
    #[error("Accessor {0} has no bufferView (may be Draco-compressed)")]
    NoBufferView(u64),
    #[error("Buffer index {0} out of range")]
    BufferOutOfRange(u64),
    #[error("Buffer read out of bounds: offset {offset}, size {size}, buffer len {buffer_len}")]
    OutOfBounds {
        offset: usize,
        size: usize,
        buffer_len: usize,
    },
    #[error("Unsupported component type: {0}")]
    UnsupportedComponentType(u32),
    #[error("Unsupported accessor type: {0}")]
    UnsupportedAccessorType(String),
    #[error("Missing required field: {0}")]
    MissingField(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentType {
    Byte = 5120,
    UnsignedByte = 5121,
    Short = 5122,
    UnsignedShort = 5123,
    UnsignedInt = 5125,
    Float = 5126,
}

impl ComponentType {
    pub fn from_u32(value: u32) -> Result<Self, Error> {
        match value {
            5120 => Ok(Self::Byte),
            5121 => Ok(Self::UnsignedByte),
            5122 => Ok(Self::Short),
            5123 => Ok(Self::UnsignedShort),
            5125 => Ok(Self::UnsignedInt),
            5126 => Ok(Self::Float),
            _ => Err(Error::UnsupportedComponentType(value)),
        }
    }

    pub fn byte_size(self) -> usize {
        match self {
            Self::Byte | Self::UnsignedByte => 1,
            Self::Short | Self::UnsignedShort => 2,
            Self::UnsignedInt | Self::Float => 4,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AccessorInfo {
    pub buffer_view_idx: u64,
    pub byte_offset: usize,
    pub component_type: ComponentType,
    pub count: usize,
    pub accessor_type: String,
}

#[derive(Debug, Clone)]
pub struct BufferViewInfo {
    pub buffer_idx: u64,
    pub byte_offset: usize,
    pub byte_length: usize,
    pub byte_stride: Option<usize>,
}

pub fn get_accessor_info(json: &Value, accessor_idx: u64) -> Result<AccessorInfo, Error> {
    let accessor = json
        .get("accessors")
        .and_then(|a| a.get(accessor_idx as usize))
        .ok_or(Error::AccessorNotFound(accessor_idx))?;

    let buffer_view_idx = accessor
        .get("bufferView")
        .and_then(|v| v.as_u64())
        .ok_or(Error::NoBufferView(accessor_idx))?;

    let byte_offset = accessor
        .get("byteOffset")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    let component_type_raw = accessor
        .get("componentType")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| Error::MissingField("accessor.componentType".into()))?
        as u32;
    let component_type = ComponentType::from_u32(component_type_raw)?;
    let count = accessor
        .get("count")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| Error::MissingField("accessor.count".into()))? as usize;
    let accessor_type = accessor
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::MissingField("accessor.type".into()))?
        .to_string();

    Ok(AccessorInfo {
        buffer_view_idx,
        byte_offset,
        component_type,
        count,
        accessor_type,
    })
}

pub fn get_buffer_view_info(json: &Value, buffer_view_idx: u64) -> Result<BufferViewInfo, Error> {
    let bv = json
        .get("bufferViews")
        .and_then(|b| b.get(buffer_view_idx as usize))
        .ok_or(Error::BufferViewNotFound(buffer_view_idx))?;

    let buffer_idx = bv
        .get("buffer")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| Error::MissingField("bufferView.buffer".into()))?;
    let byte_offset = bv.get("byteOffset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let byte_length =
        bv.get("byteLength")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| Error::MissingField("bufferView.byteLength".into()))? as usize;
    let byte_stride = bv
        .get("byteStride")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);

    Ok(BufferViewInfo {
        buffer_idx,
        byte_offset,
        byte_length,
        byte_stride,
    })
}

fn component_count(accessor_type: &str) -> Result<usize, Error> {
    match accessor_type {
        "SCALAR" => Ok(1),
        "VEC2" => Ok(2),
        "VEC3" => Ok(3),
        "VEC4" => Ok(4),
        "MAT2" => Ok(4),
        "MAT3" => Ok(9),
        "MAT4" => Ok(16),
        _ => Err(Error::UnsupportedAccessorType(accessor_type.to_string())),
    }
}

pub fn read_accessor_as_f32(
    json: &Value,
    buffer: &[u8],
    accessor_idx: u64,
) -> Result<Vec<f32>, Error> {
    let accessor = get_accessor_info(json, accessor_idx)?;
    let buffer_view = get_buffer_view_info(json, accessor.buffer_view_idx)?;

    if buffer_view.buffer_idx != 0 {
        return Err(Error::BufferOutOfRange(buffer_view.buffer_idx));
    }

    let num_components = component_count(&accessor.accessor_type)?;
    let element_size = accessor.component_type.byte_size() * num_components;
    let stride = buffer_view.byte_stride.unwrap_or(element_size);
    let base_offset = buffer_view.byte_offset + accessor.byte_offset;
    let total_floats = accessor.count * num_components;

    // Fast path: tightly-packed f32 data can be copied directly
    if accessor.component_type == ComponentType::Float && stride == element_size {
        let total_bytes = total_floats * 4;
        if base_offset + total_bytes > buffer.len() {
            return Err(Error::OutOfBounds {
                offset: base_offset,
                size: total_bytes,
                buffer_len: buffer.len(),
            });
        }
        let mut result = vec![0.0f32; total_floats];
        // Safety: copying raw LE bytes into f32 slice; both are 4-byte aligned in the Vec
        let dst =
            unsafe { std::slice::from_raw_parts_mut(result.as_mut_ptr() as *mut u8, total_bytes) };
        dst.copy_from_slice(&buffer[base_offset..base_offset + total_bytes]);
        return Ok(result);
    }

    let mut result = Vec::with_capacity(total_floats);

    for i in 0..accessor.count {
        let element_offset = base_offset + i * stride;
        for c in 0..num_components {
            let offset = element_offset + c * accessor.component_type.byte_size();
            let value = read_component_as_f32(buffer, offset, accessor.component_type)?;
            result.push(value);
        }
    }

    Ok(result)
}

pub fn read_accessor_as_u32(
    json: &Value,
    buffer: &[u8],
    accessor_idx: u64,
) -> Result<Vec<u32>, Error> {
    let accessor = get_accessor_info(json, accessor_idx)?;
    let buffer_view = get_buffer_view_info(json, accessor.buffer_view_idx)?;

    if buffer_view.buffer_idx != 0 {
        return Err(Error::BufferOutOfRange(buffer_view.buffer_idx));
    }

    let element_size = accessor.component_type.byte_size();
    let stride = buffer_view.byte_stride.unwrap_or(element_size);
    let base_offset = buffer_view.byte_offset + accessor.byte_offset;

    let mut result = Vec::with_capacity(accessor.count);

    for i in 0..accessor.count {
        let offset = base_offset + i * stride;
        let value = read_component_as_u32(buffer, offset, accessor.component_type)?;
        result.push(value);
    }

    Ok(result)
}

fn read_component_as_f32(buffer: &[u8], offset: usize, ct: ComponentType) -> Result<f32, Error> {
    let size = ct.byte_size();
    if offset + size > buffer.len() {
        return Err(Error::OutOfBounds {
            offset,
            size,
            buffer_len: buffer.len(),
        });
    }

    Ok(match ct {
        ComponentType::Byte => buffer[offset] as i8 as f32,
        ComponentType::UnsignedByte => buffer[offset] as f32,
        ComponentType::Short => i16::from_le_bytes([buffer[offset], buffer[offset + 1]]) as f32,
        ComponentType::UnsignedShort => {
            u16::from_le_bytes([buffer[offset], buffer[offset + 1]]) as f32
        }
        ComponentType::UnsignedInt => u32::from_le_bytes([
            buffer[offset],
            buffer[offset + 1],
            buffer[offset + 2],
            buffer[offset + 3],
        ]) as f32,
        ComponentType::Float => f32::from_le_bytes([
            buffer[offset],
            buffer[offset + 1],
            buffer[offset + 2],
            buffer[offset + 3],
        ]),
    })
}

fn read_component_as_u32(buffer: &[u8], offset: usize, ct: ComponentType) -> Result<u32, Error> {
    let size = ct.byte_size();
    if offset + size > buffer.len() {
        return Err(Error::OutOfBounds {
            offset,
            size,
            buffer_len: buffer.len(),
        });
    }

    Ok(match ct {
        ComponentType::Byte => buffer[offset] as i8 as u32,
        ComponentType::UnsignedByte => buffer[offset] as u32,
        ComponentType::Short => i16::from_le_bytes([buffer[offset], buffer[offset + 1]]) as u32,
        ComponentType::UnsignedShort => {
            u16::from_le_bytes([buffer[offset], buffer[offset + 1]]) as u32
        }
        ComponentType::UnsignedInt => u32::from_le_bytes([
            buffer[offset],
            buffer[offset + 1],
            buffer[offset + 2],
            buffer[offset + 3],
        ]),
        ComponentType::Float => f32::from_le_bytes([
            buffer[offset],
            buffer[offset + 1],
            buffer[offset + 2],
            buffer[offset + 3],
        ]) as u32,
    })
}

pub fn read_accessor_as_vec3(
    json: &Value,
    buffer: &[u8],
    accessor_idx: u64,
) -> Result<Vec<[f32; 3]>, Error> {
    read_accessor_as_array::<3>(json, buffer, accessor_idx)
}

pub fn read_accessor_as_vec2(
    json: &Value,
    buffer: &[u8],
    accessor_idx: u64,
) -> Result<Vec<[f32; 2]>, Error> {
    read_accessor_as_array::<2>(json, buffer, accessor_idx)
}

pub fn read_accessor_as_vec4(
    json: &Value,
    buffer: &[u8],
    accessor_idx: u64,
) -> Result<Vec<[f32; 4]>, Error> {
    read_accessor_as_array::<4>(json, buffer, accessor_idx)
}

fn read_accessor_as_array<const N: usize>(
    json: &Value,
    buffer: &[u8],
    accessor_idx: u64,
) -> Result<Vec<[f32; N]>, Error> {
    let accessor = get_accessor_info(json, accessor_idx)?;
    let buffer_view = get_buffer_view_info(json, accessor.buffer_view_idx)?;

    if buffer_view.buffer_idx != 0 {
        return Err(Error::BufferOutOfRange(buffer_view.buffer_idx));
    }

    let element_size = accessor.component_type.byte_size() * N;
    let stride = buffer_view.byte_stride.unwrap_or(element_size);
    let base_offset = buffer_view.byte_offset + accessor.byte_offset;

    // Fast path: tightly-packed f32 data can be reinterpreted directly
    if accessor.component_type == ComponentType::Float && stride == element_size {
        let total_bytes = accessor.count * N * 4;
        if base_offset + total_bytes > buffer.len() {
            return Err(Error::OutOfBounds {
                offset: base_offset,
                size: total_bytes,
                buffer_len: buffer.len(),
            });
        }
        let mut result = vec![[0.0f32; N]; accessor.count];
        let dst =
            unsafe { std::slice::from_raw_parts_mut(result.as_mut_ptr() as *mut u8, total_bytes) };
        dst.copy_from_slice(&buffer[base_offset..base_offset + total_bytes]);
        return Ok(result);
    }

    // Slow path: per-component conversion
    let mut result = Vec::with_capacity(accessor.count);
    for i in 0..accessor.count {
        let element_offset = base_offset + i * stride;
        let mut arr = [0.0f32; N];
        for (c, slot) in arr.iter_mut().enumerate() {
            let offset = element_offset + c * accessor.component_type.byte_size();
            *slot = read_component_as_f32(buffer, offset, accessor.component_type)?;
        }
        result.push(arr);
    }

    Ok(result)
}

pub fn read_accessor_as_scalar_f32(
    json: &Value,
    buffer: &[u8],
    accessor_idx: u64,
) -> Result<Vec<f32>, Error> {
    let accessor = get_accessor_info(json, accessor_idx)?;
    let buffer_view = get_buffer_view_info(json, accessor.buffer_view_idx)?;

    if buffer_view.buffer_idx != 0 {
        return Err(Error::BufferOutOfRange(buffer_view.buffer_idx));
    }

    let element_size = accessor.component_type.byte_size();
    let stride = buffer_view.byte_stride.unwrap_or(element_size);
    let base_offset = buffer_view.byte_offset + accessor.byte_offset;

    let mut result = Vec::with_capacity(accessor.count);

    for i in 0..accessor.count {
        let offset = base_offset + i * stride;
        let value = read_component_as_f32(buffer, offset, accessor.component_type)?;
        result.push(value);
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_read_vec3() {
        let json = json!({
            "accessors": [{ "bufferView": 0, "byteOffset": 0, "componentType": 5126, "count": 3, "type": "VEC3" }],
            "bufferViews": [{ "buffer": 0, "byteOffset": 0, "byteLength": 36 }]
        });

        let mut buffer = Vec::new();
        for v in [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0] {
            buffer.extend_from_slice(&v.to_le_bytes());
        }

        let positions = read_accessor_as_vec3(&json, &buffer, 0).unwrap();
        assert_eq!(positions.len(), 3);
        assert_eq!(positions[0], [1.0, 2.0, 3.0]);
        assert_eq!(positions[1], [4.0, 5.0, 6.0]);
    }
}
