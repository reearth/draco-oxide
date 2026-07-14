//! SIMD kernels (`simd` feature) with scalar fallbacks. Uses `core::arch::wasm32`
//! simd128 when compiled for wasm32 with `+simd128`; scalar elsewhere.
