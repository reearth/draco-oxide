// `impl_ndvector_ops` reads DRACO_OXIDE_MAX_VECTOR_DIM via env::var at expansion
// time, so the generated code depends on it. Cargo doesn't track env vars in its
// fingerprint by default; this directive ties a rebuild of *this* crate (the
// one that reads the var) to a change in its value.
fn main() {
    println!("cargo:rerun-if-env-changed=DRACO_OXIDE_MAX_VECTOR_DIM");
}
