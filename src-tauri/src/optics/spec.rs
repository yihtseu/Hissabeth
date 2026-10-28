use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Serialize)]
pub struct FieldFrame {
    pub sagittal: [f64; 2],
    pub tangential: [f64; 2],
}
impl FieldFrame {
    pub fn project(axis: [f64; 2], vector: [f64; 2]) -> f64 {
        axis[0] * vector[0] + axis[1] * vector[1]
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub x: f64,
    pub y: f64,
    pub weight: f64,
    pub vignetting: [f64; 4],
}
impl Field {
    pub fn meridional(y: f64) -> Self {
        Self {
            x: 0.,
            y,
            weight: 1.,
            vignetting: [0.; 4],
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wavelength {
    pub value: f64,
    pub weight: f64,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    Angle,
    ObjectHeight,
    ImageHeight,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PupilMapping {
    ReferenceEntrancePupil,
    RealStop,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PupilType {
    EntrancePupilDiameter,
    FNumber,
    ObjectNa,
    ImageNa,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pupil {
    pub kind: PupilType,
    pub value: f64,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Interaction {
    Refract,
    Reflect,
    Dummy,
}
