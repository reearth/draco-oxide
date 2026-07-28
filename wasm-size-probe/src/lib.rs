//! Minimal C-ABI surface over `draco-oxide-decoder`, used only to measure the
//! linked WASM module size of each decoder feature tier.

use draco_oxide_decoder as dec;

/// Decodes `len` bytes at `ptr` to portable (integer) attributes and returns
/// the face count, or -1 on error.
///
/// # Safety
/// `ptr` must point to `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn probe_decode_portable(ptr: *const u8, len: usize) -> i64 {
    let bytes = core::slice::from_raw_parts(ptr, len);
    match dec::decode_portable(bytes) {
        Ok(portable) => portable.mesh.faces.len() as i64,
        Err(_) => -1,
    }
}

/// Decodes `len` bytes at `ptr` to original-format attributes and returns the
/// face count, or -1 on error.
///
/// # Safety
/// `ptr` must point to `len` readable bytes.
#[cfg(feature = "dequantize")]
#[no_mangle]
pub unsafe extern "C" fn probe_decode(ptr: *const u8, len: usize) -> i64 {
    let bytes = core::slice::from_raw_parts(ptr, len);
    match dec::decode(bytes) {
        Ok(mesh) => mesh.faces.len() as i64,
        Err(_) => -1,
    }
}
