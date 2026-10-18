//! A fourth tool, written the way any new tool is: this one file, and one
//! line where the registry is built. Nothing else in the engine knows it
//! exists.
//!
//! It fades the picture towards middle grey, and its settings have been
//! through a version change, so the test also shows an old saved edit being
//! migrated.

use argentum_engine::{Context, Error, Field, Image, Result, Section, Stage, Tool};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// 0 leaves the picture alone, 1 is flat grey.
    pub amount: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self { amount: 0.0 }
    }
}

impl argentum_engine::Settings for Settings {
    const VERSION: u32 = 2;
    const FIELDS: &'static [Field] = &[Field::new("amount").range(0.0, 1.0).step(0.01)];

    /// Version 1 called it `strength` and stored a percentage.
    fn migrate(from: u32, value: Value) -> std::result::Result<Value, String> {
        match from {
            1 => {
                let pct = value.get("strength").and_then(Value::as_f64).unwrap_or(0.0);
                Ok(serde_json::json!({ "amount": pct / 100.0 }))
            }
            _ => Err(format!("no migration from version {from}")),
        }
    }
}

pub const GREY: f32 = 0.18;

#[derive(Default)]
pub struct AgDummy;

impl Tool for AgDummy {
    const ID: &'static str = "ag_dummy";
    const STAGE: Stage = Stage::SceneLinear;
    const SECTION: Section = Section::Effects;
    type Settings = Settings;

    fn process(&self, s: &Settings, input: &Image, _cx: &Context) -> Result<Option<Image>> {
        let rgb = input.as_rgb().ok_or_else(|| Error::Tool {
            tool: Self::ID,
            message: format!("was handed {}", input.describe()),
        })?;
        if s.amount == 0.0 {
            return Ok(None);
        }
        let pixels = rgb
            .pixels
            .iter()
            .map(|p| p.map(|v| v + (GREY - v) * s.amount))
            .collect();
        Ok(Some(Image::Rgb(rgb.with_pixels(pixels))))
    }
}
