use serde::{Deserialize, Serialize};
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize)]
pub struct FieldFrame {
    pub sagittal: [f64; 2],
    pub tangential: [f64; 2],
}
impl FieldFrame {
    pub fn project(axis: [f64; 2], vector: [f64; 2]) -> f64 {
        axis[0] * vector[0] + axis[1] * vector[1]
    }
}
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    #[schemars(range(min = -1e15, max = 1e15))]
    pub x: f64,
    #[schemars(range(min = -1e15, max = 1e15))]
    pub y: f64,
    #[schemars(range(min = 0.0, max = 1e15))]
    pub weight: f64,
    #[schemars(extend("items" = {"type": "number", "exclusiveMaximum": 1.0}))]
    pub vignetting: [f64; 4],
    #[schemars(inner(range(min = 0.0, max = 1.0)))]
    pub color: [f64; 4],
}
impl Field {
    pub fn meridional(y: f64) -> Self {
        Self {
            x: 0.,
            y,
            weight: 1.,
            vignetting: [0.; 4],
            color: [0.196, 0.486, 0.623, 1.],
        }
    }
}
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wavelength {
    #[schemars(range(min = 100.0, max = 20000.0))]
    pub value: f64,
    #[schemars(range(min = 0.0, max = 1e15))]
    pub weight: f64,
    #[schemars(inner(range(min = 0.0, max = 1.0)))]
    pub color: [f64; 4],
}
impl Wavelength {
    pub fn new(value: f64, weight: f64) -> Self {
        Self {
            value,
            weight,
            color: wavelength_color(value),
        }
    }
}
pub fn valid_color(color: [f64; 4]) -> bool {
    color
        .iter()
        .all(|c| c.is_finite() && (0. ..=1.).contains(c))
}
pub fn wavelength_color(nm: f64) -> [f64; 4] {
    let (r, g, b) = if nm < 440. {
        (((440. - nm) / 60.).clamp(0., 1.), 0., 1.)
    } else if nm < 490. {
        (0., (nm - 440.) / 50., 1.)
    } else if nm < 510. {
        (0., 1., (510. - nm) / 20.)
    } else if nm < 580. {
        ((nm - 510.) / 70., 1., 0.)
    } else if nm < 645. {
        (1., (645. - nm) / 65., 0.)
    } else {
        (1., 0., 0.)
    };
    let edge = if nm < 380. || nm > 780. {
        0.5
    } else if nm < 420. {
        0.5 + 0.5 * (nm - 380.) / 40.
    } else if nm > 700. {
        0.5 + 0.5 * (780. - nm) / 80.
    } else {
        1.
    };
    let component = |x: f64| (x * edge).powf(0.8) * 0.85;
    [component(r), component(g), component(b), 1.]
}
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    Angle,
    ObjectHeight,
    ImageHeight,
}
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PupilMapping {
    ReferenceEntrancePupil,
    RealStop,
}
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PupilType {
    EntrancePupilDiameter,
    FNumber,
    ObjectNa,
    ImageNa,
}
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pupil {
    pub kind: PupilType,
    #[schemars(range(min = 1e-12, max = 1e9))]
    pub value: f64,
}
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Interaction {
    Refract,
    Reflect,
    Dummy,
}
