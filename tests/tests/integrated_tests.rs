// Profile-driven integration tests generated from `tests/profiles/*.toml` by
// `build.rs`. Each TOML becomes one `#[test] fn <name>()` here.
include!(concat!(env!("OUT_DIR"), "/generated_profiles.rs"));
