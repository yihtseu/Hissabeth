// SPDX-License-Identifier: GPL-3.0-or-later
// Analysis families follow Geopter; calculations use f64 and explicit invalid-ray masks.
use super::{
    Field, FieldFrame, PupilSampling, Ray, SpectralComponent, Spectrum, System, math::*, trace,
};
use serde_json::{Value, json};

const IMAGE_SAMPLING: &str = "Planar lateral analysis grid centered at the real chief image intercept; not surface irradiance integrated over a curved detector";

#[derive(serde::Serialize, schemars::JsonSchema)]
struct FanPoint {
    pupil: f64,
    tangential_error: Option<f64>,
    sagittal_error: Option<f64>,
    error: Option<String>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct RayFan {
    tangential: Vec<FanPoint>,
    sagittal: Vec<FanPoint>,
    frame: FieldFrame,
    chief_image: [f64; 3],
    wavelength: f64,
    units: &'static str,
}

pub fn ray_fan(s: &System, f: &Field, w: f64, samples: usize) -> Result<Value, String> {
    serde_json::to_value(ray_fan_report(s, f, w, samples)?).map_err(|e| e.to_string())
}
pub(crate) fn ray_fan_report(
    s: &System,
    f: &Field,
    w: f64,
    samples: usize,
) -> Result<RayFan, String> {
    if !(3..=257).contains(&samples) {
        return Err("Fan samples must be 3–257".into());
    }
    let chief = s.trace_field(0., 0., f, w)?;
    let c = chief.image();
    let radius = s.pupil_radius(w)?;
    let frame = s.field_frame(f);
    let mut tangential = vec![];
    let mut sagittal = vec![];
    for i in 0..samples {
        let p = 2. * i as f64 / (samples - 1) as f64 - 1.;
        for (axis, out) in [
            (frame.tangential, &mut tangential),
            (frame.sagittal, &mut sagittal),
        ] {
            out.push(
                match s.trace_field(radius * p * axis[0], radius * p * axis[1], f, w) {
                    Ok(ray) => {
                        let h = ray.image();
                        let error = [h[0] - c[0], h[1] - c[1]];
                        FanPoint {
                            pupil: p,
                            tangential_error: Some(FieldFrame::project(frame.tangential, error)),
                            sagittal_error: Some(FieldFrame::project(frame.sagittal, error)),
                            error: None,
                        }
                    }
                    Err(error) => FanPoint {
                        pupil: p,
                        tangential_error: None,
                        sagittal_error: None,
                        error: Some(error),
                    },
                },
            );
        }
    }
    Ok(RayFan {
        tangential,
        sagittal,
        frame,
        chief_image: c,
        wavelength: w,
        units: "mm",
    })
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct ChromaticPoint {
    wavelength: f64,
    focus: Option<f64>,
    shift: Option<f64>,
    error: Option<String>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct ChromaticFocus {
    reference_wavelength: f64,
    reference_focus: f64,
    points: Vec<ChromaticPoint>,
    units: &'static str,
}

pub fn chromatic_focus(
    s: &System,
    lower: f64,
    upper: f64,
    samples: usize,
) -> Result<Value, String> {
    serde_json::to_value(chromatic_focus_report(s, lower, upper, samples)?)
        .map_err(|e| e.to_string())
}
pub(crate) fn chromatic_focus_report(
    s: &System,
    lower: f64,
    upper: f64,
    samples: usize,
) -> Result<ChromaticFocus, String> {
    if !lower.is_finite() || !upper.is_finite() || upper <= lower || !(2..=257).contains(&samples) {
        return Err("Invalid chromatic wavelength interval or sample count".into());
    }
    let reference = s.paraxial(s.reference())?["image_distance"]
        .as_f64()
        .ok_or("No reference focus")?;
    let mut points = vec![];
    for i in 0..samples {
        let w = lower + (upper - lower) * i as f64 / (samples - 1) as f64;
        let focus = s.paraxial(w).and_then(|p| {
            p["image_distance"]
                .as_f64()
                .ok_or_else(|| "No finite paraxial focus".into())
        });
        points.push(match focus {
            Ok(focus) => ChromaticPoint {
                wavelength: w,
                focus: Some(focus),
                shift: Some(focus - reference),
                error: None,
            },
            Err(error) => ChromaticPoint {
                wavelength: w,
                focus: None,
                shift: None,
                error: Some(error),
            },
        });
    }
    Ok(ChromaticFocus {
        reference_wavelength: s.reference(),
        reference_focus: reference,
        points,
        units: "mm",
    })
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct LongitudinalPoint {
    pupil: f64,
    focus: Option<f64>,
    shift: Option<f64>,
    error: Option<String>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct LongitudinalAberration {
    wavelength: f64,
    paraxial_focus: f64,
    points: Vec<LongitudinalPoint>,
    units: &'static str,
}

pub fn longitudinal(s: &System, w: f64, samples: usize) -> Result<Value, String> {
    serde_json::to_value(longitudinal_report(s, w, samples)?).map_err(|e| e.to_string())
}
pub(crate) fn longitudinal_report(
    s: &System,
    w: f64,
    samples: usize,
) -> Result<LongitudinalAberration, String> {
    if !(3..=257).contains(&samples) {
        return Err("Longitudinal samples must be 3–257".into());
    }
    let radius = s.pupil_radius(w)?;
    let paraxial = s.paraxial(w)?["image_distance"]
        .as_f64()
        .ok_or("No finite paraxial focus")?;
    let vertex = s
        .surfaces
        .iter()
        .take(s.surfaces.len() - 1)
        .map(|x| x.thickness)
        .sum::<f64>();
    let mut points = vec![];
    for i in 0..samples {
        let p = i as f64 / (samples - 1) as f64;
        let result = if i == 0 {
            Ok(paraxial)
        } else {
            s.trace(0., p * radius, 0., w).and_then(|r| {
                let last = r.last();
                let u = last.direction;
                if u[1].abs() < 1e-15 {
                    Err("Axial ray has no unique axis intercept".into())
                } else {
                    Ok(last.point[2] - last.point[1] * u[2] / u[1] - vertex)
                }
            })
        };
        points.push(match result {
            Ok(z) => LongitudinalPoint {
                pupil: p,
                focus: Some(z),
                shift: Some(z - paraxial),
                error: None,
            },
            Err(e) => LongitudinalPoint {
                pupil: p,
                focus: None,
                shift: None,
                error: Some(e),
            },
        });
    }
    Ok(LongitudinalAberration {
        wavelength: w,
        paraxial_focus: paraxial,
        points,
        units: "mm",
    })
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct Astigmatism {
    sagittal_focus: f64,
    tangential_focus: f64,
    sagittal_shift: Option<f64>,
    tangential_shift: Option<f64>,
    astigmatism: f64,
    chief_direction: Option<[f64; 3]>,
    frame: FieldFrame,
    units: &'static str,
}

pub fn astigmatism(s: &System, f: &Field, w: f64) -> Result<Value, String> {
    serde_json::to_value(astigmatism_report(s, f, w)?).map_err(|e| e.to_string())
}
pub(crate) fn astigmatism_report(s: &System, f: &Field, w: f64) -> Result<Astigmatism, String> {
    let pair = super::focus::chief_foci(s, f, w);
    let sagittal = pair
        .sagittal
        .focus_z
        .ok_or_else(|| pair.sagittal.error.as_ref().unwrap().message.clone())?;
    let tangential = pair
        .tangential
        .focus_z
        .ok_or_else(|| pair.tangential.error.as_ref().unwrap().message.clone())?;
    let vertex = s
        .surfaces
        .iter()
        .take(s.surfaces.len() - 1)
        .map(|v| v.thickness)
        .sum::<f64>();
    Ok(Astigmatism {
        sagittal_focus: sagittal - vertex,
        tangential_focus: tangential - vertex,
        sagittal_shift: pair.sagittal.shift,
        tangential_shift: pair.tangential.shift,
        astigmatism: tangential - sagittal,
        chief_direction: pair.chief_direction,
        frame: pair.frame,
        units: "mm",
    })
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct Distortion {
    ideal: [f64; 2],
    actual: [f64; 2],
    distortion_percent: Option<f64>,
    reference: &'static str,
    units: &'static str,
}

pub fn distortion(s: &System, f: &Field, w: f64) -> Result<Value, String> {
    serde_json::to_value(distortion_report(s, f, w)?).map_err(|e| e.to_string())
}
pub(crate) fn distortion_report(s: &System, f: &Field, w: f64) -> Result<Distortion, String> {
    let image = s.trace_field(0., 0., f, w)?.image();
    let ideal = s.paraxial_chief_image(f, w)?;
    let squared_height = ideal[0] * ideal[0] + ideal[1] * ideal[1];
    let distortion = if squared_height > 1e-24 {
        Some(((image[0] * ideal[0] + image[1] * ideal[1]) / squared_height - 1.) * 100.)
    } else {
        None
    };
    Ok(Distortion {
        ideal,
        actual: [image[0], image[1]],
        distortion_percent: distortion,
        reference: "paraxial chief-ray intercept at the stored image plane",
        units: "mm",
    })
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct EnergyPoint {
    radius: f64,
    fraction: f64,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct EncircledEnergy {
    center: [f64; 2],
    points: Vec<EnergyPoint>,
    rays: usize,
    sampling: PupilSampling,
    sampled_rays: usize,
    transmitted_weight: f64,
    normalization: &'static str,
    units: &'static str,
}

pub fn encircled_energy(
    s: &System,
    f: &Field,
    w: f64,
    sampling: PupilSampling,
    samples: usize,
) -> Result<Value, String> {
    serde_json::to_value(encircled_energy_report(s, f, w, sampling, samples)?)
        .map_err(|e| e.to_string())
}
pub(crate) fn encircled_energy_report(
    s: &System,
    f: &Field,
    w: f64,
    sampling: PupilSampling,
    samples: usize,
) -> Result<EncircledEnergy, String> {
    if !(2..=257).contains(&samples) {
        return Err("Energy samples must be 2–257".into());
    }
    let bundle = s.sample_pupil(f, w, sampling)?;
    let center = bundle.centroid()?;
    let total = bundle.transmitted_weight();
    let mut distances: Vec<_> = bundle
        .points
        .iter()
        .zip(&bundle.weights)
        .map(|(p, weight)| ((p[0] - center[0]).hypot(p[1] - center[1]), *weight))
        .collect();
    distances.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut cumulative = vec![0.];
    for (_, weight) in &distances {
        cumulative.push(cumulative.last().unwrap() + weight);
    }
    let max = distances.last().unwrap().0;
    let points: Vec<_> = (0..samples)
        .map(|i| {
            let radius = max * i as f64 / (samples - 1) as f64;
            let count = distances.partition_point(|(r, _)| *r <= radius);
            EnergyPoint {
                radius,
                fraction: cumulative[count] / total,
            }
        })
        .collect();
    Ok(EncircledEnergy {
        center,
        points,
        rays: distances.len(),
        sampling,
        sampled_rays: bundle.sampled_rays,
        transmitted_weight: total,
        normalization: "transmitted nominal-pupil area weights",
        units: "mm",
    })
}

pub struct Wavefront {
    pub values: Vec<Option<f64>>,
    pub pupils: Vec<[f64; 2]>,
    pub grid: usize,
    pub wavelength: f64,
    pub valid: usize,
    pub failed: usize,
    pub chief: Ray,
}
impl Wavefront {
    pub fn json(&self) -> Value {
        json!(self.map())
    }
    pub(crate) fn map(&self) -> WavefrontMap {
        let data: Vec<f64> = self.values.iter().flatten().copied().collect();
        let mean = data.iter().sum::<f64>() / data.len() as f64;
        let rms = (data.iter().map(|w| (w - mean).powi(2)).sum::<f64>() / data.len() as f64).sqrt();
        let lo = data.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = data.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        WavefrontMap {
            grid: self.grid,
            opd: self.values.clone(),
            pupils: self.pupils.clone(),
            rms,
            peak_to_valley: hi - lo,
            mean,
            wavelength: self.wavelength,
            units: "waves",
            valid: self.valid,
            failed: self.failed,
            reference: "sphere centered at chief ray image intercept",
        }
    }
}
struct WavefrontReference {
    chief: Ray,
    image: V3,
    radius: f64,
    wavelength: f64,
}
impl WavefrontReference {
    fn new(s: &System, f: &Field, w: f64) -> Result<Self, String> {
        let chief = s.trace_field(0., 0., f, w)?;
        let image = chief.image();
        let radius = norm(sub(image, chief.last().point));
        if radius < 1e-9 {
            return Err("Image plane too close to last surface for reference sphere".into());
        }
        Ok(Self {
            chief,
            image,
            radius,
            wavelength: w,
        })
    }
    fn sphere_distance(&self, ray: &Ray) -> Result<f64, String> {
        let d = sub(ray.last().point, self.image);
        let b = dot(d, ray.direction);
        let c = dot(d, d) - self.radius * self.radius;
        let discriminant = b * b - c;
        if discriminant < 0. {
            return Err("Ray misses reference sphere".into());
        }
        let root = discriminant.sqrt();
        let t = if b < 0. {
            c / (root - b)
        } else {
            -c / (root + b)
        };
        if t.is_finite() {
            Ok(t)
        } else {
            Err("Invalid reference sphere distance".into())
        }
    }
    fn opd(&self, s: &System, f: &Field, ray: &Ray) -> Result<f64, String> {
        let chief = &self.chief;
        let input_difference = if s.object_distance.is_some() {
            let object = s.object_point(f, self.wavelength)?;
            let a = sub(ray.points[0], object);
            let b = sub(chief.points[0], object);
            let length = norm(a) + norm(b);
            if length == 0. {
                0.
            } else {
                dot(sub(ray.points[0], chief.points[0]), add(a, b)) / length
            }
        } else {
            dot(sub(ray.points[0], chief.points[0]), chief.launch_direction)
        };
        let opd = s.object_index(self.wavelength)? * input_difference
            + (ray.last().opl - chief.last().opl)
            + chief.last().n_after * (self.sphere_distance(ray)? - self.sphere_distance(chief)?);
        let waves = opd / (self.wavelength * 1e-6);
        if waves.is_finite() {
            Ok(waves)
        } else {
            Err("Nonfinite wavefront".into())
        }
    }
}
pub fn wavefront(s: &System, f: &Field, w: f64, grid: usize) -> Result<Wavefront, String> {
    if !(5..=129).contains(&grid) {
        return Err("Wavefront grid must be 5–129".into());
    }
    let reference = WavefrontReference::new(s, f, w)?;
    let r = s.pupil_radius(w)?;
    let mut values = vec![];
    let mut pupils = vec![];
    let (mut valid, mut failed) = (0, 0);
    for i in 0..grid {
        for j in 0..grid {
            let x = (j as f64 - (grid / 2) as f64) / (grid / 2) as f64;
            let y = (i as f64 - (grid / 2) as f64) / (grid / 2) as f64;
            pupils.push([x, y]);
            if x * x + y * y > 1. {
                values.push(None);
                continue;
            }
            match s
                // The FFT lattice is calibrated to the full nominal pupil.
                // Stored ray-coordinate compression must not rescale this lattice;
                // physical apertures still mask the traced pupil directly.
                .trace_options(
                    x * r,
                    y * r,
                    f,
                    w,
                    trace::TraceOptions {
                        last: s.surfaces.len() - 1,
                        image: true,
                        apertures: true,
                        vignetting: false,
                    },
                )
                .map_err(|e| e.to_string())
                .and_then(|ray| reference.opd(s, f, &ray))
            {
                Ok(v) => {
                    values.push(Some(v));
                    valid += 1;
                }
                Err(_) => {
                    values.push(None);
                    failed += 1;
                }
            }
        }
    }
    if valid < 3 {
        return Err("Insufficient transmitted wavefront samples".into());
    }
    Ok(Wavefront {
        values,
        pupils,
        grid,
        wavelength: w,
        valid,
        failed,
        chief: reference.chief,
    })
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct OpdPoint {
    pupil: f64,
    opd: Option<f64>,
    error: Option<String>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct OpdFan {
    tangential: Vec<OpdPoint>,
    sagittal: Vec<OpdPoint>,
    frame: FieldFrame,
    units: &'static str,
    wavelength: f64,
    reference: &'static str,
}

pub fn opd_fan(s: &System, f: &Field, w: f64, samples: usize) -> Result<Value, String> {
    serde_json::to_value(opd_fan_report(s, f, w, samples)?).map_err(|e| e.to_string())
}
pub(crate) fn opd_fan_report(
    s: &System,
    f: &Field,
    w: f64,
    samples: usize,
) -> Result<OpdFan, String> {
    if !(3..=257).contains(&samples) {
        return Err("Fan samples must be 3–257".into());
    }
    let reference = WavefrontReference::new(s, f, w)?;
    let frame = s.field_frame(f);
    let radius = s.pupil_radius(w)?;
    let mut tangential = vec![];
    let mut sagittal = vec![];
    for i in 0..samples {
        let p = 2. * i as f64 / (samples - 1) as f64 - 1.;
        for (axis, out) in [
            (frame.tangential, &mut tangential),
            (frame.sagittal, &mut sagittal),
        ] {
            let result = s
                .trace_field(p * radius * axis[0], p * radius * axis[1], f, w)
                .and_then(|ray| reference.opd(s, f, &ray));
            out.push(match result {
                Ok(opd) => OpdPoint {
                    pupil: p,
                    opd: Some(opd),
                    error: None,
                },
                Err(error) => OpdPoint {
                    pupil: p,
                    opd: None,
                    error: Some(error),
                },
            });
        }
    }
    Ok(OpdFan {
        tangential,
        sagittal,
        frame,
        units: "waves",
        wavelength: w,
        reference: "sphere centered at chief ray image intercept",
    })
}

#[derive(Clone, Copy, Default)]
struct Complex {
    re: f64,
    im: f64,
}
impl Complex {
    fn mul(self, b: Self) -> Self {
        Self {
            re: self.re * b.re - self.im * b.im,
            im: self.re * b.im + self.im * b.re,
        }
    }
    fn abs2(self) -> f64 {
        self.re * self.re + self.im * self.im
    }
}
fn fft(data: &mut [Complex]) {
    let n = data.len();
    debug_assert!(n.is_power_of_two());
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            data.swap(i, j)
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -2. * std::f64::consts::PI / len as f64;
        let step = Complex {
            re: angle.cos(),
            im: angle.sin(),
        };
        for start in (0..n).step_by(len) {
            let mut phase = Complex { re: 1., im: 0. };
            for k in 0..len / 2 {
                let a = data[start + k];
                let b = data[start + k + len / 2].mul(phase);
                data[start + k] = Complex {
                    re: a.re + b.re,
                    im: a.im + b.im,
                };
                data[start + k + len / 2] = Complex {
                    re: a.re - b.re,
                    im: a.im - b.im,
                };
                phase = phase.mul(step);
            }
        }
        len *= 2;
    }
}
fn fft2(data: &mut [Complex], n: usize) {
    for row in data.chunks_mut(n) {
        fft(row)
    }
    let mut column = vec![Complex::default(); n];
    for j in 0..n {
        for i in 0..n {
            column[i] = data[i * n + j]
        }
        fft(&mut column);
        for i in 0..n {
            data[i * n + j] = column[i]
        }
    }
}
struct PsfData {
    intensity: Vec<f64>,
    n: usize,
    pixel: f64,
    strehl: f64,
    failed: usize,
    valid: usize,
    chief_image: [f64; 2],
}
struct ExitMapping {
    index: f64,
    radius: f64,
    image_distance: f64,
    chief_image: [f64; 2],
}
fn exit_mapping(s: &System, w: f64, wf: &Wavefront) -> Result<ExitMapping, String> {
    let paraxial = s.paraxial(w)?;
    let exit_z = paraxial["exit_pupil_distance"]
        .as_f64()
        .ok_or("Exit pupil at infinity")?;
    let last_vertex = s
        .surfaces
        .iter()
        .take(s.surfaces.len() - 1)
        .map(|s| s.thickness)
        .sum::<f64>();
    let exit_plane = last_vertex + exit_z;
    let chief = &wf.chief;
    let distance = (exit_plane - chief.last().point[2]) / chief.direction[2];
    let pupil_center = add(chief.last().point, scale(chief.direction, distance));
    let image_distance = norm(sub(chief.image(), pupil_center));
    let radius = paraxial["exit_pupil_radius"]
        .as_f64()
        .ok_or("Undefined exit pupil radius")?;
    let chief_image = chief.image();
    Ok(ExitMapping {
        index: chief.last().n_after,
        radius,
        image_distance,
        chief_image: [chief_image[0], chief_image[1]],
    })
}
fn psf_data(s: &System, f: &Field, w: f64, grid: usize) -> Result<PsfData, String> {
    if !(8..=128).contains(&grid) || !grid.is_power_of_two() {
        return Err("Diffraction pupil grid must be a power of two from 8 to 128".into());
    }
    let wf = wavefront(s, f, w, grid)?;
    let n = grid * 2;
    let mut amplitude = vec![Complex::default(); n * n];
    for i in 0..grid {
        for j in 0..grid {
            if let Some(opd) = wf.values[i * grid + j] {
                let phase = 2. * std::f64::consts::PI * opd;
                amplitude[(i + grid / 2) * n + j + grid / 2] = Complex {
                    re: phase.cos(),
                    im: phase.sin(),
                };
            }
        }
    }
    fft2(&mut amplitude, n);
    let mut raw: Vec<_> = amplitude.iter().map(|v| v.abs2()).collect();
    let peak = raw.iter().copied().fold(0., f64::max);
    let total = raw.iter().sum::<f64>();
    if total <= 0. {
        return Err("Zero diffraction energy".into());
    }
    for v in &mut raw {
        *v /= total
    }
    let mut centered = vec![0.; n * n];
    for i in 0..n {
        for j in 0..n {
            centered[i * n + j] = raw[((i + n / 2) % n) * n + (j + n / 2) % n]
        }
    }
    let map = exit_mapping(s, w, &wf)?;
    let pixel =
        w * 1e-6 / map.index * map.image_distance * grid as f64 / (n as f64 * 2. * map.radius);
    if !pixel.is_finite() || pixel <= 0. {
        return Err("Undefined diffraction image sampling".into());
    }
    Ok(PsfData {
        intensity: centered,
        n,
        pixel,
        strehl: peak / (wf.valid * wf.valid) as f64,
        failed: wf.failed,
        valid: wf.valid,
        chief_image: map.chief_image,
    })
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct DiffractionPsf {
    intensity: Vec<f64>,
    grid: usize,
    pixel_size: f64,
    strehl: f64,
    wavelength: f64,
    normalization: &'static str,
    model: &'static str,
    image_sampling: &'static str,
    failed_pupil_rays: usize,
    units: &'static str,
}

pub fn diffraction_psf(s: &System, f: &Field, w: f64, grid: usize) -> Result<Value, String> {
    serde_json::to_value(diffraction_psf_report(s, f, w, grid)?).map_err(|e| e.to_string())
}
pub(crate) fn diffraction_psf_report(
    s: &System,
    f: &Field,
    w: f64,
    grid: usize,
) -> Result<DiffractionPsf, String> {
    let psf = psf_data(s, f, w, grid)?;
    Ok(DiffractionPsf {
        intensity: psf.intensity,
        grid: psf.n,
        pixel_size: psf.pixel,
        strehl: psf.strehl,
        wavelength: w,
        normalization: "sum intensity = 1",
        model: "Fraunhofer with paraxial exit pupil mapping",
        image_sampling: IMAGE_SAMPLING,
        failed_pupil_rays: psf.failed,
        units: "mm",
    })
}

// Direct Huygens-Fresnel summation over the traced pupil samples, evaluated at
// arbitrary image coordinates with the same paraxial exit pupil mapping as the
// FFT path. Independent of the FFT lattice, so the two cross-validate.
fn huygens_psf_data(
    s: &System,
    f: &Field,
    w: f64,
    grid: usize,
    size: usize,
    pitch: Option<f64>,
) -> Result<PsfData, String> {
    if !(5..=129).contains(&grid) {
        return Err("Huygens pupil grid must be 5–129".into());
    }
    if !(3..=129).contains(&size) {
        return Err("Huygens image size must be 3–129".into());
    }
    let wf = wavefront(s, f, w, grid)?;
    let map = exit_mapping(s, w, &wf)?;
    let pixel = pitch.unwrap_or(w * 1e-6 / map.index * map.image_distance / (4. * map.radius));
    if !pixel.is_finite() || pixel <= 0. {
        return Err("Undefined diffraction image sampling".into());
    }
    let scale = std::f64::consts::TAU * map.index / (w * 1e-6 * map.image_distance);
    let valid_pupils: Vec<(f64, f64, f64)> = wf
        .pupils
        .iter()
        .zip(&wf.values)
        .filter_map(|(p, value)| {
            value.map(|opd| {
                (
                    p[0] * map.radius,
                    p[1] * map.radius,
                    std::f64::consts::TAU * opd,
                )
            })
        })
        .collect();
    let half = (size / 2) as f64;
    let rows: Vec<Vec<f64>> = super::parallel::par_map_index(size, 4, |i| {
        let y = (i as f64 - half) * pixel;
        let mut row_re = vec![0.; size];
        let mut row_im = vec![0.; size];
        for &(px, py, phase_opd) in &valid_pupils {
            let row = scale * y * py - phase_opd;
            for j in 0..size {
                let x = (j as f64 - half) * pixel;
                let (sin, cos) = (row + scale * x * px).sin_cos();
                row_re[j] += cos;
                row_im[j] -= sin;
            }
        }
        row_re
            .into_iter()
            .zip(row_im)
            .map(|(re, im)| re * re + im * im)
            .collect()
    });
    let mut intensity: Vec<f64> = rows.into_iter().flatten().collect();
    let peak = intensity.iter().copied().fold(0., f64::max);
    let total = intensity.iter().sum::<f64>();
    if total <= 0. {
        return Err("Zero diffraction energy".into());
    }
    for v in &mut intensity {
        *v /= total
    }
    Ok(PsfData {
        intensity,
        n: size,
        pixel,
        strehl: peak / (wf.valid * wf.valid) as f64,
        failed: wf.failed,
        valid: wf.valid,
        chief_image: map.chief_image,
    })
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct HuygensPsf {
    intensity: Vec<f64>,
    grid: usize,
    pupil_grid: usize,
    pixel_size: f64,
    strehl: f64,
    wavelength: f64,
    normalization: &'static str,
    model: &'static str,
    image_sampling: &'static str,
    failed_pupil_rays: usize,
    units: &'static str,
}

pub fn huygens_psf(
    s: &System,
    f: &Field,
    w: f64,
    grid: usize,
    size: Option<usize>,
    pitch: Option<f64>,
) -> Result<Value, String> {
    serde_json::to_value(huygens_psf_report(s, f, w, grid, size, pitch)?).map_err(|e| e.to_string())
}
pub(crate) fn huygens_psf_report(
    s: &System,
    f: &Field,
    w: f64,
    grid: usize,
    size: Option<usize>,
    pitch: Option<f64>,
) -> Result<HuygensPsf, String> {
    let size = size.unwrap_or(2 * grid);
    let psf = huygens_psf_data(s, f, w, grid, size, pitch)?;
    Ok(HuygensPsf {
        intensity: psf.intensity,
        grid: psf.n,
        pupil_grid: grid,
        pixel_size: psf.pixel,
        strehl: psf.strehl,
        wavelength: w,
        normalization: "sum intensity = 1",
        model: "direct Huygens-Fresnel pupil sum with paraxial exit pupil mapping",
        image_sampling: IMAGE_SAMPLING,
        failed_pupil_rays: psf.failed,
        units: "mm",
    })
}
pub fn huygens_mtf(
    s: &System,
    f: &Field,
    spectrum: Spectrum,
    grid: usize,
    max_frequency: Option<f64>,
) -> Result<Value, String> {
    serde_json::to_value(huygens_mtf_report(s, f, spectrum, grid, max_frequency)?)
        .map_err(|e| e.to_string())
}
pub(crate) fn huygens_mtf_report(
    s: &System,
    f: &Field,
    spectrum: Spectrum,
    grid: usize,
    max_frequency: Option<f64>,
) -> Result<DiffractionMtf, String> {
    let mut components = vec![];
    for wave in spectrum.wavelengths(s)? {
        let data = huygens_psf_data(s, f, wave.value, grid, 2 * grid, None)
            .map_err(|e| format!("{} nm: {e}", wave.value))?;
        let fraction = data.valid as f64 / (data.valid + data.failed) as f64;
        let spectral = SpectralComponent {
            wavelength: wave.value,
            weight: wave.weight,
            color: wave.color,
            pupil_samples: data.valid + data.failed,
            transmitted_samples: data.valid,
            transmitted_fraction: fraction,
            effective_weight: wave.weight * fraction,
        };
        components.push(PsfComponent { data, spectral });
    }
    let mut result =
        compose_diffraction_mtf(&components, spectrum, s.field_frame(f), grid, max_frequency)?;
    result.model = "direct Huygens-Fresnel pupil sum with paraxial exit pupil mapping; complex OTF sum on common physical frequencies, preserving chief-ray lateral color";
    Ok(result)
}

// The Fourier transform is evaluated on a common physical frequency grid.
// Each PSF retains its own pitch and chief intercept; magnitudes are taken only
// after summing the weighted complex components.
fn psf_otf_cut(psf: &PsfData, axis: [f64; 2], shift: f64, frequencies: &[f64]) -> Vec<Complex> {
    let mut out = vec![Complex::default(); frequencies.len()];
    let cutoff = 1. / (2. * psf.pixel);
    let valid = frequencies.partition_point(|f| *f <= cutoff * (1. + 1e-12));
    let step = frequencies.get(1).copied().unwrap_or(0.);
    let norm = psf.intensity.iter().sum::<f64>();
    let mut projected = vec![];
    if axis[1] == 0. {
        let mut columns = vec![0.; psf.n];
        for (i, weight) in psf.intensity.iter().enumerate() {
            columns[i % psf.n] += weight;
        }
        projected.extend(columns.into_iter().enumerate().map(|(i, weight)| {
            (
                ((i as f64 - (psf.n / 2) as f64) * psf.pixel * axis[0] + shift),
                weight / norm,
            )
        }));
    } else if axis[0] == 0. {
        projected.extend(psf.intensity.chunks(psf.n).enumerate().map(|(i, row)| {
            (
                ((i as f64 - (psf.n / 2) as f64) * psf.pixel * axis[1] + shift),
                row.iter().sum::<f64>() / norm,
            )
        }));
    } else {
        projected.extend(psf.intensity.iter().enumerate().map(|(i, weight)| {
            let x = (i % psf.n) as f64 - (psf.n / 2) as f64;
            let y = (i / psf.n) as f64 - (psf.n / 2) as f64;
            (
                psf.pixel * (x * axis[0] + y * axis[1]) + shift,
                *weight / norm,
            )
        }));
    }
    for (position, weight) in projected {
        let angle = std::f64::consts::TAU * step * position;
        let (sin, cos) = angle.sin_cos();
        let increment = Complex { re: cos, im: -sin };
        let mut phase = Complex { re: 1., im: 0. };
        for sample in out.iter_mut().take(valid) {
            sample.re += weight * phase.re;
            sample.im += weight * phase.im;
            phase = phase.mul(increment);
        }
    }
    out
}
struct PsfComponent {
    data: PsfData,
    spectral: SpectralComponent,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct Otf {
    pub real: Vec<f64>,
    pub imaginary: Vec<f64>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct DiffractionComponent {
    #[serde(flatten)]
    spectral: SpectralComponent,
    pixel_size: f64,
    sampling_cutoff: f64,
    chief_image: [f64; 2],
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct DiffractionMtf {
    frequencies: Vec<f64>,
    sagittal: Vec<f64>,
    tangential: Vec<f64>,
    sagittal_otf: Otf,
    tangential_otf: Otf,
    spectrum: Spectrum,
    components: Vec<DiffractionComponent>,
    origin: [f64; 2],
    transmitted_weight: f64,
    pupil_grid: usize,
    sampling_limit: f64,
    max_frequency: f64,
    frame: FieldFrame,
    units: &'static str,
    image_sampling: &'static str,
    failed_pupil_rays: usize,
    normalization: &'static str,
    model: &'static str,
}

fn compose_diffraction_mtf(
    components: &[PsfComponent],
    spectrum: Spectrum,
    frame: FieldFrame,
    grid: usize,
    max_frequency: Option<f64>,
) -> Result<DiffractionMtf, String> {
    let total = components
        .iter()
        .map(|c| c.spectral.effective_weight)
        .sum::<f64>();
    if total <= 0. {
        return Err("No positive-weight diffraction energy".into());
    }
    let origin = [0, 1].map(|axis| {
        components
            .iter()
            .map(|c| c.data.chief_image[axis] * (c.spectral.effective_weight / total))
            .sum::<f64>()
    });
    let limit = components
        .iter()
        .map(|c| 1. / (2. * c.data.pixel))
        .fold(0_f64, f64::max);
    let max = max_frequency.unwrap_or(limit);
    if !max.is_finite() || max <= 0. || max > limit * (1. + 1e-12) {
        return Err(format!(
            "max_frequency must be positive and no greater than the diffraction sampling limit {limit} cycles/mm"
        ));
    }
    let frequencies: Vec<_> = (0..=grid).map(|i| max * i as f64 / grid as f64).collect();
    let [(sagittal, sagittal_otf), (tangential, tangential_otf)] =
        [frame.sagittal, frame.tangential].map(|axis| {
            let mut combined = vec![Complex::default(); frequencies.len()];
            for component in components {
                let offset = [
                    component.data.chief_image[0] - origin[0],
                    component.data.chief_image[1] - origin[1],
                ];
                let shift = FieldFrame::project(axis, offset);
                let cut = psf_otf_cut(&component.data, axis, shift, &frequencies);
                let weight = component.spectral.effective_weight / total;
                for (sum, value) in combined.iter_mut().zip(cut) {
                    sum.re += weight * value.re;
                    sum.im += weight * value.im;
                }
            }
            let otf = Otf {
                real: combined.iter().map(|v| v.re).collect(),
                imaginary: combined.iter().map(|v| v.im).collect(),
            };
            (combined.iter().map(|v| v.abs2().sqrt()).collect(), otf)
        });
    let spectral = components
        .iter()
        .map(|c| DiffractionComponent {
            spectral: c.spectral.clone(),
            pixel_size: c.data.pixel,
            sampling_cutoff: 1. / (2. * c.data.pixel),
            chief_image: c.data.chief_image,
        })
        .collect();
    Ok(DiffractionMtf {
        frequencies,
        sagittal,
        tangential,
        sagittal_otf,
        tangential_otf,
        spectrum,
        components: spectral,
        origin,
        transmitted_weight: total,
        pupil_grid: grid,
        sampling_limit: limit,
        max_frequency: max,
        frame,
        units: "cycles/mm",
        image_sampling: IMAGE_SAMPLING,
        failed_pupil_rays: components.iter().map(|c| c.data.failed).sum::<usize>(),
        normalization: "incident spectral weights times sampled pupil transmission; normalized at DC",
        model: "Fraunhofer with paraxial exit pupil mapping; complex OTF sum on common physical frequencies, preserving chief-ray lateral color",
    })
}
pub fn diffraction_mtf(
    s: &System,
    f: &Field,
    spectrum: Spectrum,
    grid: usize,
    max_frequency: Option<f64>,
) -> Result<Value, String> {
    serde_json::to_value(diffraction_mtf_report(s, f, spectrum, grid, max_frequency)?)
        .map_err(|e| e.to_string())
}
pub(crate) fn diffraction_mtf_report(
    s: &System,
    f: &Field,
    spectrum: Spectrum,
    grid: usize,
    max_frequency: Option<f64>,
) -> Result<DiffractionMtf, String> {
    let mut components = vec![];
    for wave in spectrum.wavelengths(s)? {
        let data =
            psf_data(s, f, wave.value, grid).map_err(|e| format!("{} nm: {e}", wave.value))?;
        let fraction = data.valid as f64 / (data.valid + data.failed) as f64;
        let spectral = SpectralComponent {
            wavelength: wave.value,
            weight: wave.weight,
            color: wave.color,
            pupil_samples: data.valid + data.failed,
            transmitted_samples: data.valid,
            transmitted_fraction: fraction,
            effective_weight: wave.weight * fraction,
        };
        components.push(PsfComponent { data, spectral });
    }
    compose_diffraction_mtf(&components, spectrum, s.field_frame(f), grid, max_frequency)
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct WavefrontMap {
    grid: usize,
    opd: Vec<Option<f64>>,
    pupils: Vec<[f64; 2]>,
    rms: f64,
    peak_to_valley: f64,
    mean: f64,
    wavelength: f64,
    units: &'static str,
    valid: usize,
    failed: usize,
    reference: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chromatic_zero_at_reference() {
        let s = System::default();
        let w = s.reference();
        let r = chromatic_focus(&s, w - 10., w + 10., 3).unwrap();
        assert!(r["points"][1]["shift"].as_f64().unwrap().abs() < 1e-12);
    }
    #[test]
    fn differential_focus_matches_paraxial_on_axis() {
        let s = System::default();
        let p = s.paraxial(s.reference()).unwrap();
        let a = astigmatism(&s, &Field::meridional(0.), s.reference()).unwrap();
        assert!((a["sagittal_focus"].as_f64().unwrap() - p["bfl"].as_f64().unwrap()).abs() < 1e-5);
        assert!(a["astigmatism"].as_f64().unwrap().abs() < 1e-8);
    }
    #[test]
    fn wavefront_chief_is_zero() {
        let s = System::default();
        let wf = wavefront(&s, &Field::meridional(0.), s.reference(), 17).unwrap();
        assert!(wf.values[8 * 17 + 8].unwrap().abs() < 1e-9);
    }
    #[test]
    fn psf_energy_and_mtf_dc() {
        let s = System::default();
        let psf = diffraction_psf(&s, &Field::meridional(0.), s.reference(), 16).unwrap();
        let energy = psf["intensity"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap())
            .sum::<f64>();
        assert!((energy - 1.).abs() < 1e-12);
        let mtf = diffraction_mtf(
            &s,
            &Field::meridional(0.),
            Spectrum::Monochromatic {
                wavelength: s.reference(),
            },
            16,
            None,
        )
        .unwrap();
        assert!((mtf["sagittal"][0].as_f64().unwrap() - 1.).abs() < 1e-12);
        assert!(
            mtf["tangential"]
                .as_array()
                .unwrap()
                .iter()
                .all(|v| v.as_f64().unwrap() <= 1.00000001)
        );
    }
    #[test]
    fn huygens_matches_fft_psf_and_mtf() {
        let s = System::default();
        let f = Field::meridional(3.);
        let w = s.reference();
        let fft = diffraction_psf(&s, &f, w, 32).unwrap();
        let huygens = huygens_psf(&s, &f, w, 32, None, None).unwrap();
        assert_eq!(fft["grid"], huygens["grid"]);
        assert!(
            (fft["pixel_size"].as_f64().unwrap() - huygens["pixel_size"].as_f64().unwrap()).abs()
                < 1e-15
        );
        let peak = fft["intensity"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .fold(0., f64::max);
        for (a, b) in fft["intensity"]
            .as_array()
            .unwrap()
            .iter()
            .zip(huygens["intensity"].as_array().unwrap())
        {
            let a = a.as_f64().unwrap();
            let b = b.as_f64().unwrap();
            assert!((a - b).abs() < 1e-9 * peak, "psf {a} vs {b}");
        }
        assert!(
            (fft["strehl"].as_f64().unwrap() - huygens["strehl"].as_f64().unwrap()).abs() < 1e-9
        );
        let spectrum = Spectrum::Monochromatic { wavelength: w };
        let a = diffraction_mtf(&s, &f, spectrum, 16, None).unwrap();
        let b = huygens_mtf(&s, &f, spectrum, 16, None).unwrap();
        for axis in ["sagittal", "tangential"] {
            for (x, y) in a[axis]
                .as_array()
                .unwrap()
                .iter()
                .zip(b[axis].as_array().unwrap())
            {
                let x = x.as_f64().unwrap();
                let y = y.as_f64().unwrap();
                assert!((x - y).abs() < 1e-9, "mtf {axis}: {x} vs {y}");
            }
        }
    }
    #[test]
    fn fft_known_impulse() {
        let mut v = vec![Complex::default(); 8];
        v[0] = Complex { re: 1., im: 0. };
        fft(&mut v);
        assert!(
            v.iter()
                .all(|z| (z.re - 1.).abs() < 1e-12 && z.im.abs() < 1e-12)
        );
    }
    #[test]
    fn fan_masks_blocked_rays() {
        let mut s = System::default();
        s.surfaces[1].aperture = 1.;
        let fan = ray_fan(&s, &Field::meridional(0.), s.reference(), 9).unwrap();
        assert!(
            fan["tangential"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["tangential_error"].is_null() && p["error"].is_string())
        );
    }
    #[test]
    fn rotated_fields_preserve_local_fans_opd_and_differential_focus() {
        let s = System::default();
        let f = Field::meridional(3.);
        let mut rotated = f.clone();
        let slope = 3_f64.to_radians().tan();
        rotated.x = (0.6 * slope).atan().to_degrees();
        rotated.y = (0.8 * slope).atan().to_degrees();
        for analyze in [ray_fan, opd_fan] {
            let a = analyze(&s, &f, s.reference(), 9).unwrap();
            let b = analyze(&s, &rotated, s.reference(), 9).unwrap();
            for axis in ["tangential", "sagittal"] {
                for (a, b) in a[axis]
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(b[axis].as_array().unwrap())
                {
                    for key in ["opd", "tangential_error", "sagittal_error"] {
                        if let Some(a) = a[key].as_f64() {
                            let b = b[key].as_f64().unwrap();
                            assert!((a - b).abs() < 1e-6, "{axis}/{key}: {a} vs {b}");
                        }
                    }
                }
            }
        }
        let a = astigmatism(&s, &f, s.reference()).unwrap();
        let b = astigmatism(&s, &rotated, s.reference()).unwrap();
        for key in ["sagittal_focus", "tangential_focus"] {
            assert!((a[key].as_f64().unwrap() - b[key].as_f64().unwrap()).abs() < 1e-4);
        }
    }
    #[test]
    fn finite_distortion_uses_stored_image_plane_and_physical_object() {
        use super::super::FieldType;
        let mut s = System {
            object_distance: Some(200.),
            ..Default::default()
        };
        s.pupil.value = 0.1;
        for kind in [
            FieldType::Angle,
            FieldType::ObjectHeight,
            FieldType::ImageHeight,
        ] {
            s.field_type = kind;
            s.fields = vec![Field::meridional(1e-4)];
            for image_distance in [60., 80.] {
                s.surfaces.last_mut().unwrap().thickness = image_distance;
                s.update().unwrap();
                let f = &s.fields[0];
                let object = s.object_point(f, s.reference()).unwrap();
                let ep = s.pupil_matrix(s.reference()).unwrap();
                let slope = -object[1] / (200. + ep.b / ep.a);
                let (m, n) = s.matrix(s.reference(), 0, s.surfaces.len()).unwrap();
                let height = object[1] + 200. * slope;
                let expected =
                    m.a * height + m.b * slope + image_distance / n * (m.c * height + m.d * slope);
                let result = distortion(&s, f, s.reference()).unwrap();
                assert!((result["ideal"][1].as_f64().unwrap() - expected).abs() < 1e-12);
                assert!(
                    result["distortion_percent"].as_f64().unwrap().abs() < 1e-4,
                    "{kind:?}: {result}"
                );
            }
        }
    }
    #[test]
    fn finite_chief_reference_matches_closed_form_thin_lens_geometry() {
        use super::super::FieldType;
        let mut s = System::default();
        s.surfaces[0].thickness = 0.;
        s.surfaces[1].thickness = 100.;
        s.object_distance = Some(150.);
        s.pupil.value = 0.1;
        let n = s
            .material_index(&s.surfaces[0].material, s.reference())
            .unwrap();
        let power = (n - 1.) * (1. / 50. - 1. / -50.);
        let magnification = -1. / (power * 150. - 1.);
        // The stop and both thin-lens vertices coincide. A chief ray has zero
        // height there, so its image height is -image_distance*object_height/d.
        // This oracle uses no matrix, pupil-matrix, or object-point implementation.
        for (kind, field_height, object_height) in [
            (FieldType::Angle, (0.3_f64 / 150.).atan().to_degrees(), -0.3),
            (FieldType::ObjectHeight, -0.3, -0.3),
            (FieldType::ImageHeight, 0.1, 0.1 / magnification),
        ] {
            s.field_type = kind;
            let f = Field::meridional(field_height);
            for w in [s.reference(), 486.1327] {
                let result = distortion(&s, &f, w).unwrap();
                let expected = -100. * object_height / 150.;
                assert!(
                    (result["ideal"][1].as_f64().unwrap() - expected).abs() < 1e-11,
                    "{kind:?}: {result}"
                );
                assert!((result["actual"][1].as_f64().unwrap() - expected).abs() < 1e-11);
            }
        }
    }
    #[test]
    fn local_mtf_cut_matches_anisotropic_gaussian_transform() {
        let n = 64;
        let psf: Vec<_> = (0..n * n)
            .map(|i| {
                let x = (i % n) as f64 - 32.;
                let y = (i / n) as f64 - 32.;
                (-0.5 * (x * x / 9. + y * y / 36.)).exp()
            })
            .collect();
        let mut otf: Vec<_> = psf.iter().map(|v| Complex { re: *v, im: 0. }).collect();
        fft2(&mut otf, n);
        for axis in [[1., 0.], [0., 1.], [0.6, 0.8], [-0.8, 0.6]] {
            let data = PsfData {
                intensity: psf.clone(),
                n,
                pixel: 1.,
                strehl: 1.,
                failed: 0,
                valid: n * n,
                chief_image: [0., 0.],
            };
            let frequencies: Vec<_> = (0..=n / 2).map(|k| k as f64 / n as f64).collect();
            let cut = psf_otf_cut(&data, axis, 0., &frequencies)
                .iter()
                .map(|v| v.abs2().sqrt())
                .collect::<Vec<_>>();
            for (k, c) in cut.iter().enumerate().take(8) {
                let variance = 9. * axis[0] * axis[0] + 36. * axis[1] * axis[1];
                let expected =
                    (-2. * std::f64::consts::PI.powi(2) * (k as f64 / n as f64).powi(2) * variance)
                        .exp();
                assert!(
                    (c - expected).abs() < 2e-6,
                    "{axis:?}/{k}: {c} vs {expected}"
                );
            }
        }
    }
    #[test]
    fn very_near_finite_object_keeps_forward_launch_and_finite_opd() {
        let mut s = System {
            object_distance: Some(0.1),
            ..Default::default()
        };
        s.pupil.value = 0.002;
        s.fields = vec![Field::meridional(0.)];
        s.update().unwrap();
        let ray = s.trace_field(0., 0., &s.fields[0], s.reference()).unwrap();
        assert!(ray.points[0][2] >= -0.1);
        let wf = wavefront(&s, &s.fields[0], s.reference(), 9).unwrap();
        assert!(wf.values[40].unwrap().abs() < 1e-9);
        assert!(wf.values.iter().flatten().all(|v| v.is_finite()));
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct Transmission {
    wavelength: f64,
    weight: f64,
    color: [f64; 4],
    transmitted: usize,
    rejected: usize,
    transmitted_fraction: f64,
    effective_weight: f64,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct PolychromaticSpot {
    #[serde(flatten)]
    spot: super::sampling::Spot,
    wavelengths: Vec<f64>,
    colors: Vec<[f64; 4]>,
    failures: Vec<Transmission>,
}

pub fn polychromatic_spot(s: &System, f: &Field, sampling: PupilSampling) -> Result<Value, String> {
    serde_json::to_value(polychromatic_spot_report(s, f, sampling)?).map_err(|e| e.to_string())
}
pub(crate) fn polychromatic_spot_report(
    s: &System,
    f: &Field,
    sampling: PupilSampling,
) -> Result<PolychromaticSpot, String> {
    let (bundle, components) = s.sample_spectrum(f, sampling, Spectrum::Polychromatic {})?;
    let spot = bundle.spot()?;
    let wavelengths: Vec<_> = components
        .iter()
        .flat_map(|component| {
            std::iter::repeat_n(component.wavelength, component.transmitted_samples)
        })
        .collect();
    let colors: Vec<_> = components
        .iter()
        .flat_map(|component| std::iter::repeat_n(component.color, component.transmitted_samples))
        .collect();
    let failures: Vec<_> = components
        .iter()
        .map(|component| Transmission {
            wavelength: component.wavelength,
            weight: component.weight,
            color: component.color,
            transmitted: component.transmitted_samples,
            rejected: component.pupil_samples - component.transmitted_samples,
            transmitted_fraction: component.transmitted_fraction,
            effective_weight: component.effective_weight,
        })
        .collect();
    Ok(PolychromaticSpot {
        spot,
        wavelengths,
        colors,
        failures,
    })
}

#[cfg(test)]
mod physical_tests {
    use super::*;
    #[test]
    fn finite_object_wavefront_matches_uniform_medium_sphere_intersection() {
        use crate::optics::{FieldType, Interaction, Surface, Wavelength};
        for image_distance in [10., -20.] {
            let mut s = System {
                object_material: "1.500000:60.000000".into(),
                wavelengths: vec![Wavelength::new(587.562, 1.)],
                reference_wavelength: 0,
                object_distance: Some(100.),
                field_type: FieldType::ObjectHeight,
                fields: vec![Field::meridional(0.)],
                ..Default::default()
            };
            s.pupil.value = 2.;
            let mut surface = Surface::sphere(0., image_distance, "AIR", 20.);
            surface.interaction = Interaction::Dummy;
            s.surfaces = vec![surface];
            s.update().unwrap();
            let wf = wavefront(&s, &s.fields[0], s.reference(), 9).unwrap();
            // Intersect an object-centered straight line with the reference sphere.
            let theta = (0.5_f64 / 100.).atan();
            let b = (100. + image_distance) * theta.cos();
            let root = (image_distance * image_distance
                - ((100. + image_distance) * theta.sin()).powi(2))
            .sqrt();
            let distance =
                100. * (100. + 2. * image_distance) / (b + image_distance.signum() * root);
            let expected = 1.5 * (distance - 100.) / (587.562e-6);
            assert!((wf.values[6 * 9 + 4].unwrap() - expected).abs() < 5e-8);
        }
    }
    #[test]
    fn circular_pupil_mtf_matches_overlap_formula() {
        let n = 128;
        let diameter = 64.;
        let mut data = vec![Complex::default(); n * n];
        for i in 0..n {
            for j in 0..n {
                let x = j as f64 - 64.;
                let y = i as f64 - 64.;
                if x * x + y * y <= (diameter / 2_f64).powi(2) {
                    data[i * n + j] = Complex { re: 1., im: 0. };
                }
            }
        }
        fft2(&mut data, n);
        for x in &mut data {
            *x = Complex {
                re: x.abs2(),
                im: 0.,
            };
        }
        fft2(&mut data, n);
        let dc = data[0].abs2().sqrt();
        for k in [8, 16, 24, 32, 40, 48, 56] {
            let nu = k as f64 / diameter;
            let expected = 2. / std::f64::consts::PI * (nu.acos() - nu * (1. - nu * nu).sqrt());
            let actual = data[k].abs2().sqrt() / dc;
            assert!(
                (actual - expected).abs() < 0.012,
                "frequency {nu}: {actual} versus {expected}"
            );
        }
    }
    #[test]
    fn spherical_wavefront_scales_as_fourth_order_at_paraxial_focus() {
        let mut s = System::default();
        s.surfaces.last_mut().unwrap().thickness =
            s.paraxial(s.reference()).unwrap()["bfl"].as_f64().unwrap();
        s.pupil.value = 4.;
        let a = wavefront(&s, &Field::meridional(0.), s.reference(), 9)
            .unwrap()
            .values[4 * 9 + 8]
            .unwrap()
            .abs();
        s.pupil.value = 2.;
        let b = wavefront(&s, &Field::meridional(0.), s.reference(), 9)
            .unwrap()
            .values[4 * 9 + 8]
            .unwrap()
            .abs();
        assert!(
            (a / b - 16.).abs() < 0.6,
            "Quartic pupil scaling: {}",
            a / b
        );
    }
}

#[cfg(test)]
mod spectral_tests {
    use super::*;
    use crate::optics::{PupilMapping, Surface, Wavelength};
    fn impulse_component(
        wavelength: f64,
        weight: f64,
        fraction: f64,
        pixel: f64,
        y: f64,
    ) -> PsfComponent {
        let n = 8;
        let mut intensity = vec![0.; n * n];
        intensity[4 * n + 4] = 1.;
        let valid = (fraction * 10.) as usize;
        PsfComponent {
            data: PsfData {
                intensity,
                n,
                pixel,
                strehl: 1.,
                valid,
                failed: 10 - valid,
                chief_image: [0., y],
            },
            spectral: SpectralComponent {
                wavelength,
                weight,
                color: crate::optics::wavelength_color(wavelength),
                pupil_samples: 10,
                transmitted_samples: valid,
                transmitted_fraction: fraction,
                effective_weight: weight * fraction,
            },
        }
    }
    #[test]
    fn complex_psf_composition_preserves_lateral_offsets_flux_and_individual_cutoffs() {
        let frame = FieldFrame {
            sagittal: [1., 0.],
            tangential: [0., 1.],
        };
        let mut components = vec![
            impulse_component(500., 1., 1., 0.001, -0.005),
            impulse_component(700., 1., 1., 0.002, 0.005),
        ];
        let result = compose_diffraction_mtf(
            &components,
            Spectrum::Polychromatic {},
            frame,
            8,
            Some(100.),
        )
        .unwrap();
        assert!(result.tangential[4] < 1e-12);
        assert!((result.sagittal[4] - 1.).abs() < 1e-12);
        components[1].spectral.weight = 3.;
        components[1].spectral.effective_weight = 3.;
        let result = compose_diffraction_mtf(
            &components,
            Spectrum::Polychromatic {},
            frame,
            8,
            Some(100.),
        )
        .unwrap();
        assert!((result.tangential[4] - 0.5).abs() < 1e-12);
        components[0] = impulse_component(500., 1., 0.5, 0.001, -0.005);
        let result =
            compose_diffraction_mtf(&components, Spectrum::Polychromatic {}, frame, 8, None)
                .unwrap();
        assert_eq!(result.sampling_limit, 500.);
        // At 312.5 cycles/mm the long-wavelength component is beyond its cutoff;
        // its DFT must not wrap around and re-enter the weighted response.
        assert!((result.sagittal[5] - 0.5 / 3.5).abs() < 1e-12);
        assert!(
            compose_diffraction_mtf(
                &components,
                Spectrum::Polychromatic {},
                frame,
                8,
                Some(501.)
            )
            .is_err()
        );
        assert!(
            compose_diffraction_mtf(&components, Spectrum::Polychromatic {}, frame, 8, Some(0.))
                .is_err()
        );
    }
    fn overlap(nu: f64) -> f64 {
        if nu >= 1. {
            0.
        } else {
            2. / std::f64::consts::PI * (nu.acos() - nu * (1. - nu * nu).sqrt())
        }
    }
    #[test]
    fn full_diffraction_pipeline_matches_circular_aperture_otf_at_low_na() {
        let mut s = System {
            fields: vec![Field::meridional(0.)],
            ..Default::default()
        };
        s.pupil.value = 0.05;
        s.wavelengths = vec![Wavelength::new(486.1327, 1.), Wavelength::new(656.2725, 3.)];
        s.reference_wavelength = 0;
        s.surfaces[1].thickness = s.paraxial(s.reference()).unwrap()["bfl"].as_f64().unwrap();
        s.update().unwrap();
        let result =
            diffraction_mtf(&s, &s.fields[0], Spectrum::Polychromatic {}, 64, None).unwrap();
        let components = result["components"].as_array().unwrap();
        for index in [0, 8, 16, 24, 32, 40, 48, 56, 64] {
            let frequency = result["frequencies"][index].as_f64().unwrap();
            let expected = components
                .iter()
                .map(|c| {
                    c["weight"].as_f64().unwrap()
                        * overlap(frequency / c["sampling_cutoff"].as_f64().unwrap())
                })
                .sum::<f64>()
                / 4.;
            for axis in ["sagittal", "tangential"] {
                let actual = result[axis][index].as_f64().unwrap();
                assert!(
                    (actual - expected).abs() < 0.025,
                    "{axis}/{frequency}: {actual} vs circular-aperture {expected}"
                );
            }
        }
        assert!((result["transmitted_weight"].as_f64().unwrap() - 4.).abs() < 1e-12);
    }
    #[test]
    fn monochromatic_diffraction_mtf_matches_fft_bins_without_rescaling() {
        let s = System::default();
        let f = Field::meridional(0.);
        let w = s.reference();
        let data = psf_data(&s, &f, w, 16).unwrap();
        let mut otf: Vec<_> = data
            .intensity
            .iter()
            .map(|v| Complex { re: *v, im: 0. })
            .collect();
        fft2(&mut otf, data.n);
        let dc = otf[0].abs2().sqrt();
        let result =
            diffraction_mtf(&s, &f, Spectrum::Monochromatic { wavelength: w }, 16, None).unwrap();
        for i in 0..=16 {
            assert!(
                (result["sagittal"][i].as_f64().unwrap() - otf[i].abs2().sqrt() / dc).abs() < 1e-12
            );
            assert!(
                (result["tangential"][i].as_f64().unwrap() - otf[i * data.n].abs2().sqrt() / dc)
                    .abs()
                    < 1e-12
            );
        }
    }
    #[test]
    fn fft_pupil_ignores_ray_compression_and_retains_physical_clipping() {
        for mapping in [PupilMapping::RealStop, PupilMapping::ReferenceEntrancePupil] {
            let mut s = System {
                pupil_mapping: mapping,
                ..Default::default()
            };
            s.pupil.value = 2.;
            s.fields = vec![Field {
                x: 0.8,
                y: 0.6,
                weight: 1.,
                vignetting: [0.; 4],
                color: [0.2, 0.5, 0.8, 1.],
            }];
            s.surfaces[0].aperture = 0.65;
            s.update().unwrap();
            let wave = wavefront(&s, &s.fields[0], s.reference(), 16).unwrap();
            assert!(wave.failed > 0 && wave.valid > 3);
            let psf = diffraction_psf(&s, &s.fields[0], s.reference(), 16).unwrap();
            let mtf =
                diffraction_mtf(&s, &s.fields[0], Spectrum::Polychromatic {}, 16, None).unwrap();
            s.fields[0].vignetting = [0.4, 0.2, -0.1, 0.3];
            s.update().unwrap();
            let compressed = wavefront(&s, &s.fields[0], s.reference(), 16).unwrap();
            assert_eq!(wave.values, compressed.values);
            assert_eq!(wave.failed, compressed.failed);
            assert_eq!(wave.valid, compressed.valid);
            assert_eq!(
                psf,
                diffraction_psf(&s, &s.fields[0], s.reference(), 16).unwrap()
            );
            assert_eq!(
                mtf,
                diffraction_mtf(&s, &s.fields[0], Spectrum::Polychromatic {}, 16, None).unwrap()
            );
        }
    }
    #[test]
    fn spectral_diffraction_skips_zero_weight_and_reports_positive_weight_failure() {
        let mut s = System {
            fields: vec![Field::meridional(0.)],
            wavelengths: vec![Wavelength::new(587.5618, 2.), Wavelength::new(435.834, 0.)],
            reference_wavelength: 0,
            ..Default::default()
        };
        // Catalog N-BK7 has a wavelength range; the default model glass has none.
        s.surfaces[0].material = "N-BK7_SCHOTT".into();
        s.update().unwrap();
        let a =
            diffraction_mtf(&s, &s.fields[0], Spectrum::Polychromatic {}, 16, Some(50.)).unwrap();
        let b = diffraction_mtf(
            &s,
            &s.fields[0],
            Spectrum::Monochromatic {
                wavelength: s.reference(),
            },
            16,
            Some(50.),
        )
        .unwrap();
        assert_eq!(a["sagittal"], b["sagittal"]);
        assert_eq!(a["components"].as_array().unwrap().len(), 1);
        assert!(
            diffraction_mtf(
                &s,
                &s.fields[0],
                Spectrum::Monochromatic { wavelength: 3000. },
                16,
                None
            )
            .unwrap_err()
            .contains("3000")
        );
        let mut s = System {
            stop_index: 1,
            ..Default::default()
        };
        s.pupil.value = 0.00001;
        s.fields = vec![Field::meridional(5.)];
        s.wavelengths = vec![Wavelength::new(486.1327, 1.), Wavelength::new(656.2725, 1.)];
        s.reference_wavelength = 0;
        s.update().unwrap();
        let a = s
            .trace_field(0., 0., &s.fields[0], s.wavelengths[0].value)
            .unwrap()
            .segments[0]
            .point[1]
            .abs();
        let b = s
            .trace_field(0., 0., &s.fields[0], s.wavelengths[1].value)
            .unwrap()
            .segments[0]
            .point[1]
            .abs();
        s.surfaces[0].aperture = (a + b) / 2.;
        s.update().unwrap();
        assert!(
            diffraction_mtf(&s, &s.fields[0], Spectrum::Polychromatic {}, 16, None)
                .unwrap_err()
                .contains("blocked")
        );
    }
    #[test]
    fn encircled_energy_integrates_area_weights_instead_of_ray_counts() {
        let mut s = System {
            surfaces: vec![Surface::sphere(0., 10., "AIR", 2.)],
            fields: vec![Field::meridional(0.)],
            ..Default::default()
        };
        s.pupil.value = 2.;
        s.update().unwrap();
        let result = encircled_energy(
            &s,
            &s.fields[0],
            s.reference(),
            PupilSampling::Hexapolar { rings: 2 },
            5,
        )
        .unwrap();
        assert!((result["points"][1]["fraction"].as_f64().unwrap() - 0.0625).abs() < 1e-12);
        assert!((result["points"][3]["fraction"].as_f64().unwrap() - 0.5625).abs() < 1e-12);
        assert!((result["points"][4]["fraction"].as_f64().unwrap() - 1.).abs() < 1e-12);
        assert_eq!(result["rays"], 19);
    }
}
