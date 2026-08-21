//! What each preset setting is, so the console can offer a form instead of raw JSON.
//!
//! Mirrored from the desktop application's `survey/preset_schema.py`, its
//! `_KEY_LABELS` table and `models/cache.py`. Only publishable keys appear: the two
//! path settings name files on one machine's disk, so the registry refuses to store
//! them. The desktop's `coerce_settings` drops names it cannot offer, so a stale copy
//! of this table degrades to a missing dropdown entry, never a broken laptop.

use serde::Serialize;

/// Bumped whenever a field or choice below changes, so the console and the devices
/// can be told apart by which schema they carry.
pub const PRESET_SCHEMA_VERSION: i32 = 1;

/// The two settings the desktop marks `publishable = False`: paths on one machine's
/// disk, meaningless and misleading anywhere else.
pub const UNPUBLISHABLE_KEYS: &[&str] = &["loger_model_path", "scs_checkpoint_path"];

/// What a field accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Int,
    Float,
    Bool,
    Enum,
}

/// A field's bundled default, serialised as the plain JSON value.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(untagged)]
pub enum Default {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(&'static str),
    Null,
}

/// One run setting: what it accepts, and what the interface calls it.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct PresetField {
    pub key: &'static str,
    /// Plain-language name, the desktop's `_KEY_LABELS` spelling.
    pub label: &'static str,
    pub kind: Kind,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub step: Option<f64>,
    pub decimals: Option<u32>,
    pub unit: &'static str,
    /// Which entry of `choices` supplies the legal values, for `kind == Enum`.
    pub choices: &'static str,
    /// The `mapping_name` values this field is used with, empty meaning always.
    pub applies_when: &'static [&'static str],
    /// Null means "the model's own size" for the two processing dimensions.
    pub nullable: bool,
    pub default: Default,
}

const fn field(
    key: &'static str,
    label: &'static str,
    kind: Kind,
    default: Default,
) -> PresetField {
    PresetField {
        key,
        label,
        kind,
        minimum: None,
        maximum: None,
        step: None,
        decimals: None,
        unit: "",
        choices: "",
        applies_when: &[],
        nullable: false,
        default,
    }
}

const fn number(
    key: &'static str,
    label: &'static str,
    kind: Kind,
    minimum: f64,
    maximum: f64,
    default: Default,
) -> PresetField {
    let mut out = field(key, label, kind, default);
    out.minimum = Some(minimum);
    out.maximum = Some(maximum);
    out
}

const fn choice(
    key: &'static str,
    label: &'static str,
    choices: &'static str,
    default: Default,
) -> PresetField {
    let mut out = field(key, label, Kind::Enum, default);
    out.choices = choices;
    out
}

/// Every publishable setting, in the order the form shows them.
// Values mirror the desktop's `preset_schema.py` table and `survey_preset.yaml`
// defaults; a drift here is a dropdown offering what no laptop accepts.
pub const FIELDS: &[PresetField] = &[
    number(
        "fps",
        "frames per second",
        Kind::Int,
        1.0,
        60.0,
        Default::Int(5),
    ),
    choice(
        "segmentation_name",
        "coral identification model",
        "segmentation",
        Default::Str("coralscapes-vit-b-dpt"),
    ),
    choice(
        "mapping_name",
        "processing method",
        "mapping",
        Default::Str("loger_star"),
    ),
    choice(
        "camera_profile_name",
        "camera",
        "camera",
        Default::Str("gopro_hero_10"),
    ),
    // Zero disables the crop, which is why the floor is 0 rather than a width.
    PresetField {
        step: Some(0.1),
        decimals: Some(2),
        unit: "m",
        ..number(
            "transect_crop_width",
            "transect width",
            Kind::Float,
            0.0,
            50.0,
            Default::Float(1.0),
        )
    },
    field(
        "enable_tsdf",
        "surface fusion",
        Kind::Bool,
        Default::Bool(false),
    ),
    field(
        "skip_segmentation",
        "skipping coral identification",
        Kind::Bool,
        Default::Bool(false),
    ),
    choice(
        "resolution_preset",
        "image resolution",
        "resolution",
        Default::Str("Native"),
    ),
    // Null means the model's own native size, so these two are optional.
    PresetField {
        step: Some(32.0),
        nullable: true,
        ..number(
            "processing_width",
            "image width",
            Kind::Int,
            256.0,
            3840.0,
            Default::Null,
        )
    },
    PresetField {
        step: Some(32.0),
        nullable: true,
        ..number(
            "processing_height",
            "image height",
            Kind::Int,
            256.0,
            2160.0,
            Default::Null,
        )
    },
    number(
        "preprocess_batch_size",
        "frames processed at once",
        Kind::Int,
        1.0,
        16.0,
        Default::Int(4),
    ),
    PresetField {
        step: Some(100.0),
        ..number(
            "grid_bins",
            "map detail",
            Kind::Int,
            100.0,
            10000.0,
            Default::Int(2000),
        )
    },
    field(
        "require_gravity_telemetry",
        "requiring camera tilt data",
        Kind::Bool,
        Default::Bool(false),
    ),
    PresetField {
        step: Some(0.1),
        decimals: Some(2),
        ..number(
            "replacement_radius_factor",
            "replacement radius factor",
            Kind::Float,
            0.0,
            10.0,
            Default::Float(0.0),
        )
    },
    number(
        "replacement_radius_estimation_frames",
        "replacement radius estimation frames",
        Kind::Int,
        1.0,
        200.0,
        Default::Int(30),
    ),
    PresetField {
        step: Some(0.001),
        decimals: Some(4),
        unit: "m",
        ..number(
            "replacement_radius_override",
            "replacement radius override",
            Kind::Float,
            0.0,
            10.0,
            Default::Float(0.0),
        )
    },
    PresetField {
        applies_when: &["loger", "loger_star"],
        ..number(
            "loger_window_size",
            "loger window size",
            Kind::Int,
            1.0,
            256.0,
            Default::Int(32),
        )
    },
    PresetField {
        applies_when: &["loger", "loger_star"],
        ..number(
            "loger_overlap_size",
            "loger overlap size",
            Kind::Int,
            0.0,
            64.0,
            Default::Int(3),
        )
    },
    field(
        "refine_intrinsics_from_mapper",
        "camera lens refinement",
        Kind::Bool,
        Default::Bool(false),
    ),
    PresetField {
        step: Some(32.0),
        applies_when: &["scsfmlearner"],
        ..number(
            "scs_target_width",
            "scs target width",
            Kind::Int,
            64.0,
            2048.0,
            Default::Int(512),
        )
    },
    PresetField {
        step: Some(32.0),
        applies_when: &["scsfmlearner"],
        ..number(
            "scs_target_height",
            "scs target height",
            Kind::Int,
            64.0,
            2048.0,
            Default::Int(256),
        )
    },
];

/// One model the desktop can run, described well enough to pick from a list.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ModelChoice {
    pub name: &'static str,
    pub description: &'static str,
    /// Hugging Face repositories the desktop downloads for it.
    pub hf_repos: &'static [&'static str],
    /// Needs a Hugging Face login before it downloads.
    pub gated: bool,
    /// Refuses to run without CUDA, so a CPU-only laptop cannot take it.
    pub gpu_only: bool,
    pub approx_size_mb: Option<u32>,
}

/// The segmentation models the desktop ships, mirrored from `models/cache.py`.
pub const SEGMENTATION_MODELS: &[ModelChoice] = &[
    ModelChoice {
        name: "segformer-b2",
        description: "SegFormer B2 (lightweight, no auth required)",
        hf_repos: &["EPFL-ECEO/segformer-b2-finetuned-coralscapes-1024-1024"],
        gated: false,
        gpu_only: false,
        approx_size_mb: Some(110),
    },
    ModelChoice {
        name: "segformer-b5",
        description: "SegFormer B5 (larger, no auth required)",
        hf_repos: &["EPFL-ECEO/segformer-b5-finetuned-coralscapes-1024-1024"],
        gated: false,
        gpu_only: false,
        approx_size_mb: Some(339),
    },
    ModelChoice {
        name: "coralscapes-vit-s-dpt",
        description: "DINOv3 ViT-S DPT (requires HF login)",
        hf_repos: &[
            "EPFL-ECEO/coralscapes-vit-s-dpt",
            "facebook/dinov3-vits16-pretrain-lvd1689m",
        ],
        gated: true,
        gpu_only: false,
        approx_size_mb: Some(257),
    },
    ModelChoice {
        name: "coralscapes-vit-b-dpt",
        description: "DINOv3 ViT-B DPT (requires HF login)",
        hf_repos: &[
            "EPFL-ECEO/coralscapes-vit-b-dpt",
            "facebook/dinov3-vitb16-pretrain-lvd1689m",
        ],
        gated: true,
        gpu_only: false,
        approx_size_mb: Some(786),
    },
    ModelChoice {
        name: "coralscapes-vit-l-dpt",
        description: "DINOv3 ViT-L DPT (largest, requires HF login)",
        hf_repos: &[
            "EPFL-ECEO/coralscapes-vit-l-dpt",
            "facebook/dinov3-vitl16-pretrain-lvd1689m",
        ],
        gated: true,
        gpu_only: false,
        approx_size_mb: Some(2542),
    },
];

/// The mapping backends the desktop ships, mirrored from `models/cache.py`.
pub const MAPPING_MODELS: &[ModelChoice] = &[
    ModelChoice {
        name: "scsfmlearner",
        description: "SC-SfMLearner depth + pose estimation",
        hf_repos: &["EPFL-ECEO/deepreefmap-sfm-net"],
        gated: false,
        gpu_only: false,
        approx_size_mb: Some(326),
    },
    ModelChoice {
        name: "loger",
        description: "LoGeR depth + pose estimation (GPU required)",
        hf_repos: &["Junyi42/LoGeR"],
        gated: false,
        gpu_only: true,
        approx_size_mb: Some(4787),
    },
    ModelChoice {
        name: "loger_star",
        description: "LoGeR* (longer-context variant, GPU required)",
        hf_repos: &["Junyi42/LoGeR"],
        gated: false,
        gpu_only: true,
        approx_size_mb: Some(4787),
    },
];

/// The camera profiles bundled with the pipeline.
pub const CAMERA_PROFILES: &[&str] = &["gopro_hero_10"];

/// Display sizes, fixed because the desktop's own combo is fixed.
pub const RESOLUTION_PRESETS: &[&str] = &["Native", "Half", "Quarter", "Custom"];

/// The legal values of one enumeration, or an empty slice for an unknown name.
#[must_use]
pub fn choices_for(name: &str) -> Vec<&'static str> {
    match name {
        "segmentation" => SEGMENTATION_MODELS.iter().map(|m| m.name).collect(),
        "mapping" => MAPPING_MODELS.iter().map(|m| m.name).collect(),
        "camera" => CAMERA_PROFILES.to_vec(),
        "resolution" => RESOLUTION_PRESETS.to_vec(),
        _ => Vec::new(),
    }
}

/// Refuse a settings document the desktop could not apply.
///
/// Unknown keys pass: clients ignore what they do not recognise, by contract. Known
/// keys must carry a value of the right kind inside the field's range, and the two
/// path keys are refused whatever they carry, because a stored path would land on
/// another machine's disk.
///
/// # Errors
///
/// Returns a message naming the first offending key.
pub fn validate_settings(settings: &serde_json::Value) -> Result<(), String> {
    let Some(map) = settings.as_object() else {
        return Err("settings must be a JSON object".to_string());
    };
    for key in UNPUBLISHABLE_KEYS {
        if map.get(*key).is_some_and(|v| !v.is_null()) {
            return Err(format!(
                "{key} is a path on one machine and cannot be published"
            ));
        }
    }
    for field in FIELDS {
        let Some(value) = map.get(field.key) else {
            continue;
        };
        validate_value(field, value)?;
    }
    Ok(())
}

fn validate_value(field: &PresetField, value: &serde_json::Value) -> Result<(), String> {
    if value.is_null() {
        if field.nullable {
            return Ok(());
        }
        return Err(format!("{} must not be null", field.key));
    }
    match field.kind {
        Kind::Bool => {
            if value.is_boolean() {
                Ok(())
            } else {
                Err(format!("{} must be true or false", field.key))
            }
        }
        Kind::Enum => {
            let name = value
                .as_str()
                .ok_or_else(|| format!("{} must be a name", field.key))?;
            let allowed = choices_for(field.choices);
            if allowed.contains(&name) {
                Ok(())
            } else {
                Err(format!(
                    "{} must be one of {}, not {name:?}",
                    field.key,
                    allowed.join(", ")
                ))
            }
        }
        Kind::Int | Kind::Float => validate_number(field, value),
    }
}

fn validate_number(field: &PresetField, value: &serde_json::Value) -> Result<(), String> {
    let number = value
        .as_f64()
        .ok_or_else(|| format!("{} must be a number", field.key))?;
    if field.kind == Kind::Int && number.fract() != 0.0 {
        return Err(format!("{} must be a whole number", field.key));
    }
    if let Some(minimum) = field.minimum
        && number < minimum
    {
        return Err(format!("{} must be at least {minimum}", field.key));
    }
    if let Some(maximum) = field.maximum
        && number > maximum
    {
        return Err(format!("{} must be at most {maximum}", field.key));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The bundled defaults must pass their own schema, or the seeded preset would be
    /// unpublishable through the console.
    #[test]
    fn test_defaults_validate() {
        let mut settings = serde_json::Map::new();
        for field in FIELDS {
            settings.insert(
                field.key.to_string(),
                serde_json::to_value(field.default).expect("a default serialises"),
            );
        }
        validate_settings(&serde_json::Value::Object(settings)).expect("the defaults are legal");
    }

    #[test]
    fn test_every_enum_field_names_a_real_enumeration() {
        for field in FIELDS {
            if field.kind == Kind::Enum {
                assert!(
                    !choices_for(field.choices).is_empty(),
                    "{} names the unknown enumeration {:?}",
                    field.key,
                    field.choices
                );
            }
        }
    }

    #[test]
    fn test_field_keys_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for field in FIELDS {
            assert!(seen.insert(field.key), "{} repeats", field.key);
        }
    }

    #[test]
    fn test_unpublishable_keys_are_refused() {
        let err = validate_settings(&json!({"loger_model_path": "/home/x/latest.pt"}))
            .expect_err("a path is refused");
        assert!(err.contains("loger_model_path"), "{err}");
        validate_settings(&json!({"loger_model_path": null})).expect("an explicit null passes");
    }

    #[test]
    fn test_unknown_model_name_is_refused() {
        let err = validate_settings(&json!({"segmentation_name": "coralscapes-vit-xl-dpt"}))
            .expect_err("an unavailable model is refused");
        assert!(err.contains("segmentation_name"), "{err}");
    }

    #[test]
    fn test_out_of_range_number_is_refused() {
        let err = validate_settings(&json!({"fps": 0})).expect_err("0 fps is refused");
        assert!(err.contains("at least 1"), "{err}");
        let err = validate_settings(&json!({"fps": "5"})).expect_err("a string is refused");
        assert!(err.contains("must be a number"), "{err}");
        let err = validate_settings(&json!({"fps": 2.5})).expect_err("a fraction is refused");
        assert!(err.contains("whole number"), "{err}");
    }

    #[test]
    fn test_null_is_only_for_the_processing_dimensions() {
        validate_settings(&json!({"processing_width": null})).expect("width may be null");
        let err = validate_settings(&json!({"fps": null})).expect_err("fps may not");
        assert!(err.contains("fps"), "{err}");
    }

    #[test]
    fn test_unknown_keys_pass() {
        validate_settings(&json!({"a_future_setting": 12})).expect("clients ignore unknown keys");
    }

    #[test]
    fn test_settings_must_be_an_object() {
        assert!(validate_settings(&json!([1, 2])).is_err());
    }
}
