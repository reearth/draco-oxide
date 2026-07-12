//! Parses the encoder `Config` from TOML to exercise the grown `ConfigSpec`
//! surface (connectivity, edgebreaker traversal, per-attribute overrides).

use draco_oxide::encode::Config;

#[test]
fn parses_rich_config_and_validates() {
    let toml = r#"
        metadata = true
        connectivity = "Edgebreaker"

        [edgebreaker]
        traversal = "Valence"

        [attributes.Position]
        prediction = "MeshParallelogramPrediction"
        quantization = { bits = 14 }

        [attributes.Normal]
        encoding = "PredictedOnly"
        quantization = { bits = 8 }
    "#;
    let cfg: Config = toml::from_str(toml).expect("valid TOML config");
    cfg.validate().expect("config validates");
}

#[test]
fn empty_toml_equals_default() {
    let cfg: Config = toml::from_str("").expect("empty TOML config");
    cfg.validate().expect("default validates");
}

#[test]
fn backcompat_normal_shortcut_still_parses() {
    let cfg: Config = toml::from_str(r#"normal = "PredictedOnly""#).expect("shortcut parses");
    cfg.validate().unwrap();
}

#[test]
fn invalid_combo_parses_but_fails_validation() {
    // A texture predictor on the Normal attribute parses fine but is rejected
    // by validate().
    let toml = r#"
        [attributes.Normal]
        prediction = "MeshPredictionForTextureCoordinates"
    "#;
    let cfg: Config = toml::from_str(toml).expect("parses");
    assert!(cfg.validate().is_err());
}

#[test]
fn unknown_field_is_rejected() {
    assert!(toml::from_str::<Config>("bogus_field = 3").is_err());
}
