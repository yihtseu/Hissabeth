// SPDX-License-Identifier: GPL-3.0-or-later
// Normalized nominal-pupil quadratures for geometric ray analyses.
use super::*;

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PupilSampling {
    Grid {
        #[schemars(range(min = 3, max = 129))]
        count: usize,
    },
    Hexapolar {
        #[schemars(range(min = 1, max = 64))]
        rings: usize,
    },
}
#[derive(Clone, Copy, Debug)]
pub struct PupilPoint {
    pub xy: [f64; 2],
    pub weight: f64,
}
impl PupilSampling {
    pub fn points(self) -> Result<Vec<PupilPoint>, String> {
        let mut points = vec![];
        match self {
            Self::Grid { count } => {
                if !(3..=129).contains(&count) {
                    return Err("Pupil grid count must be 3–129".into());
                }
                for i in 0..count {
                    for j in 0..count {
                        let x = 2. * i as f64 / (count - 1) as f64 - 1.;
                        let y = 2. * j as f64 / (count - 1) as f64 - 1.;
                        if x * x + y * y <= 1. {
                            points.push(PupilPoint {
                                xy: [x, y],
                                weight: 1.,
                            });
                        }
                    }
                }
                let count = points.len() as f64;
                for point in &mut points {
                    point.weight /= count;
                }
            }
            Self::Hexapolar { rings } => {
                if !(1..=64).contains(&rings) {
                    return Err("Hexapolar rings must be 1–64".into());
                }
                let step = 1. / rings as f64;
                points.push(PupilPoint {
                    xy: [0., 0.],
                    weight: (step / 2.).powi(2),
                });
                for ring in 1..=rings {
                    let radius = ring as f64 * step;
                    let inner = radius - step / 2.;
                    let outer = (radius + step / 2.).min(1.);
                    let count = 6 * ring;
                    let weight = (outer * outer - inner * inner) / count as f64;
                    for azimuth in 0..count {
                        let angle = std::f64::consts::TAU * azimuth as f64 / count as f64;
                        let (sin, cos) = angle.sin_cos();
                        points.push(PupilPoint {
                            xy: [radius * cos, radius * sin],
                            weight,
                        });
                    }
                }
            }
        }
        Ok(points)
    }
}

#[derive(Debug)]
pub struct RayBundle {
    pub sampling: PupilSampling,
    pub points: Vec<[f64; 2]>,
    pub weights: Vec<f64>,
    pub sampled_rays: usize,
    pub blocked: usize,
    pub missed: usize,
    pub total_internal_reflection: usize,
}
impl RayBundle {
    pub fn transmitted_weight(&self) -> f64 {
        self.weights.iter().sum()
    }
    pub fn centroid(&self) -> Result<[f64; 2], String> {
        let total = self.transmitted_weight();
        if total <= 0. {
            return Err("No rays reached image plane".into());
        }
        let mut center = [0.; 2];
        for (point, weight) in self.points.iter().zip(&self.weights) {
            for i in 0..2 {
                center[i] += point[i] * weight / total;
            }
        }
        Ok(center)
    }
    pub fn json(&self) -> Result<Value, String> {
        serde_json::to_value(self.spot()?).map_err(|e| e.to_string())
    }
    pub(crate) fn spot(&self) -> Result<Spot, String> {
        let center = self.centroid()?;
        let mut variance = 0.;
        let mut radius = 0_f64;
        for (point, weight) in self.points.iter().zip(&self.weights) {
            let r = (point[0] - center[0]).hypot(point[1] - center[1]);
            variance += weight * r * r;
            radius = radius.max(r);
        }
        Ok(Spot {
            sampling: self.sampling,
            points: self.points.clone(),
            weights: self.weights.clone(),
            sampled_rays: self.sampled_rays,
            transmitted_weight: self.transmitted_weight(),
            centroid: center,
            rms_radius: (variance / self.transmitted_weight()).sqrt(),
            geometric_radius: radius,
            blocked: self.blocked,
            missed: self.missed,
            total_internal_reflection: self.total_internal_reflection,
            backend: "cpu",
            units: "mm",
        })
    }
}
impl System {
    pub fn sample_pupil(
        &self,
        field: &Field,
        w: f64,
        sampling: PupilSampling,
    ) -> Result<RayBundle, String> {
        let pupils = sampling.points()?;
        // Invalid material/environment data is a failed analysis, not lost pupil
        // energy that can disappear from an otherwise successful spectrum.
        self.matrix(w, 0, self.surfaces.len())?;
        let radius = self.pupil_radius(w)?;
        let mut bundle = RayBundle {
            sampling,
            points: vec![],
            weights: vec![],
            sampled_rays: pupils.len(),
            blocked: 0,
            missed: 0,
            total_internal_reflection: 0,
        };
        for pupil in pupils {
            match self.trace_options(
                radius * pupil.xy[0],
                radius * pupil.xy[1],
                field,
                w,
                trace::TraceOptions {
                    last: self.surfaces.len() - 1,
                    image: true,
                    apertures: true,
                    // Area quadrature integrates the nominal pupil; physical
                    // apertures provide its transmission mask (as in Geopter).
                    vignetting: false,
                },
            ) {
                Ok(ray) => {
                    let image = ray.image();
                    bundle.points.push([image[0], image[1]]);
                    bundle.weights.push(pupil.weight);
                }
                Err(failure) => match failure.kind {
                    trace::FailureKind::Blocked => bundle.blocked += 1,
                    trace::FailureKind::TotalInternalReflection => {
                        bundle.total_internal_reflection += 1
                    }
                    _ => bundle.missed += 1,
                },
            }
        }
        Ok(bundle)
    }
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Spectrum {
    Monochromatic {
        #[schemars(range(min = 100.0, max = 20000.0))]
        wavelength: f64,
    },
    Polychromatic {},
}
impl Spectrum {
    pub fn wavelengths(self, system: &System) -> Result<Vec<Wavelength>, String> {
        let waves = match self {
            Self::Monochromatic { wavelength } => vec![Wavelength::new(wavelength, 1.)],
            Self::Polychromatic {} => system
                .wavelengths
                .iter()
                .filter(|wave| wave.weight > 0.)
                .cloned()
                .collect(),
        };
        if waves.is_empty()
            || waves.iter().any(|wave| {
                !wave.value.is_finite()
                    || !(100. ..=20000.).contains(&wave.value)
                    || !wave.weight.is_finite()
                    || wave.weight <= 0.
            })
        {
            return Err("Spectrum needs positive-weight wavelengths in 100–20000 nm".into());
        }
        Ok(waves)
    }
}

#[derive(schemars::JsonSchema, Clone, Debug, Serialize)]
pub struct SpectralComponent {
    pub wavelength: f64,
    pub weight: f64,
    pub color: [f64; 4],
    pub pupil_samples: usize,
    pub transmitted_samples: usize,
    pub transmitted_fraction: f64,
    pub effective_weight: f64,
}
impl SpectralComponent {
    pub fn from_bundle(wave: &Wavelength, bundle: &RayBundle) -> Self {
        let transmitted_fraction = bundle.transmitted_weight();
        Self {
            wavelength: wave.value,
            weight: wave.weight,
            color: wave.color,
            pupil_samples: bundle.sampled_rays,
            transmitted_samples: bundle.points.len(),
            transmitted_fraction,
            effective_weight: wave.weight * transmitted_fraction,
        }
    }
}

impl System {
    pub fn sample_spectrum(
        &self,
        field: &Field,
        sampling: PupilSampling,
        spectrum: Spectrum,
    ) -> Result<(RayBundle, Vec<SpectralComponent>), String> {
        let mut combined = RayBundle {
            sampling,
            points: vec![],
            weights: vec![],
            sampled_rays: 0,
            blocked: 0,
            missed: 0,
            total_internal_reflection: 0,
        };
        let mut components = vec![];
        for wave in spectrum.wavelengths(self)? {
            let bundle = self
                .sample_pupil(field, wave.value, sampling)
                .map_err(|e| format!("{} nm: {e}", wave.value))?;
            components.push(SpectralComponent::from_bundle(&wave, &bundle));
            combined.sampled_rays += bundle.sampled_rays;
            combined.blocked += bundle.blocked;
            combined.missed += bundle.missed;
            combined.total_internal_reflection += bundle.total_internal_reflection;
            combined.points.extend(bundle.points);
            combined.weights.extend(
                bundle
                    .weights
                    .into_iter()
                    .map(|weight| wave.weight * weight),
            );
        }
        Ok((combined, components))
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct Spot {
    sampling: PupilSampling,
    points: Vec<[f64; 2]>,
    weights: Vec<f64>,
    sampled_rays: usize,
    transmitted_weight: f64,
    centroid: [f64; 2],
    rms_radius: f64,
    geometric_radius: f64,
    blocked: usize,
    missed: usize,
    total_internal_reflection: usize,
    backend: &'static str,
    units: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_wavelengths_keep_per_entry_plot_colors_and_layout_field_indices() {
        let mut s = planes(2.);
        let a = [0.125, 0.25, 0.875, 0.];
        let b = [0.625, 0.75, 0.375, 0.5];
        s.wavelengths = vec![
            Wavelength {
                color: a,
                ..Wavelength::new(587.562, 1.)
            },
            Wavelength {
                color: b,
                ..Wavelength::new(587.562, 2.)
            },
            Wavelength::new(435.834, 0.),
        ];
        s.fields = vec![
            Field {
                color: a,
                ..Field::meridional(0.)
            },
            Field {
                color: b,
                ..Field::meridional(0.)
            },
        ];
        s.reference_wavelength = 0;
        s.update().unwrap();
        let poly =
            analysis::polychromatic_spot(&s, &s.fields[0], PupilSampling::Hexapolar { rings: 2 })
                .unwrap();
        let colors = poly["colors"].as_array().unwrap();
        assert_eq!(colors.len(), 38);
        assert!(colors[..19].iter().all(|c| *c == json!(a)));
        assert!(colors[19..].iter().all(|c| *c == json!(b)));
        assert_eq!(poly["failures"][0]["color"], json!(a));
        assert_eq!(poly["failures"][1]["color"], json!(b));
        let layout = s.layout();
        assert_eq!(
            layout["field_indices"],
            json!([0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1])
        );
        assert_eq!(layout["rays"].as_array().unwrap().len(), 14);
    }
    #[test]
    fn hexapolar_annuli_have_unit_area_symmetry_and_known_radial_moment() {
        for rings in [1, 2, 8, 64] {
            let points = PupilSampling::Hexapolar { rings }.points().unwrap();
            assert_eq!(points.len(), 1 + 3 * rings * (rings + 1));
            assert!((points.iter().map(|p| p.weight).sum::<f64>() - 1.).abs() < 2e-12);
            let centroid =
                [0, 1].map(|axis| points.iter().map(|p| p.weight * p.xy[axis]).sum::<f64>());
            assert!(centroid.iter().all(|v| v.abs() < 1e-13));
            let moment = points
                .iter()
                .map(|p| p.weight * (p.xy[0] * p.xy[0] + p.xy[1] * p.xy[1]))
                .sum::<f64>();
            assert!((moment - (0.5 + 1. / (4. * (rings * rings) as f64))).abs() < 2e-12);
            assert!(
                points
                    .iter()
                    .all(|p| p.xy[0].hypot(p.xy[1]) <= 1. + 1e-14 && p.weight > 0.)
            );
        }
    }
    #[test]
    fn sampling_and_spectrum_definitions_are_strict() {
        for value in [
            json!({"kind":"grid","count":17,"rings":2}),
            json!({"kind":"grid"}),
            json!({"kind":"hexapolar","rings":2,"count":17}),
            json!({"kind":"hexapolar","rings":1.5}),
        ] {
            assert!(serde_json::from_value::<PupilSampling>(value).is_err());
        }
        for sampling in [
            PupilSampling::Grid { count: 2 },
            PupilSampling::Grid { count: 130 },
            PupilSampling::Hexapolar { rings: 0 },
            PupilSampling::Hexapolar { rings: 65 },
        ] {
            assert!(sampling.points().is_err());
        }
        for value in [
            json!({"kind":"polychromatic","wavelength":500}),
            json!({"kind":"monochromatic"}),
            json!({"kind":"monochromatic","wavelength":500,"weight":1}),
        ] {
            assert!(serde_json::from_value::<Spectrum>(value).is_err());
        }
        let grid = PupilSampling::Grid { count: 7 }.points().unwrap();
        assert_eq!(grid.len(), 29);
        assert!(grid.iter().any(|p| p.xy == [0., 0.]));
        assert!((grid.iter().map(|p| p.weight).sum::<f64>() - 1.).abs() < 1e-12);
    }
    fn planes(aperture: f64) -> System {
        let mut s = System {
            surfaces: vec![
                Surface::sphere(0., 10., "AIR", 2.),
                Surface::sphere(0., 10., "AIR", aperture),
            ],
            fields: vec![Field::meridional(0.)],
            ..Default::default()
        };
        s.pupil.value = 2.;
        s.update().unwrap();
        s
    }
    #[test]
    fn area_weighted_spots_and_clipping_match_annular_geometry() {
        let s = planes(2.);
        let sampling = PupilSampling::Hexapolar { rings: 4 };
        let spot = s.spot_field(&s.fields[0], s.reference(), sampling).unwrap();
        assert!((spot["rms_radius"].as_f64().unwrap().powi(2) - (0.5 + 1. / 64.)).abs() < 1e-12);
        let s = planes(0.6);
        let spot = s.spot_field(&s.fields[0], s.reference(), sampling).unwrap();
        assert_eq!(spot["points"].as_array().unwrap().len(), 19);
        assert_eq!(spot["blocked"], 42);
        assert_eq!(spot["sampled_rays"], 61);
        assert!((spot["transmitted_weight"].as_f64().unwrap() - 0.625_f64.powi(2)).abs() < 1e-12);
        let weights = spot["weights"].as_array().unwrap();
        assert!(
            (weights.iter().map(|v| v.as_f64().unwrap()).sum::<f64>()
                - spot["transmitted_weight"].as_f64().unwrap())
            .abs()
                < 1e-12
        );
    }
    #[test]
    fn zero_weight_wavelength_is_not_traced_and_spectral_power_is_retained() {
        let mut s = System {
            fields: vec![Field::meridional(0.)],
            wavelengths: vec![
                Wavelength::new(587.5618, 1.),
                Wavelength::new(587.5618, 3.),
                Wavelength::new(435.834, 0.),
            ],
            reference_wavelength: 0,
            ..Default::default()
        };
        s.update().unwrap();
        let sampling = PupilSampling::Hexapolar { rings: 3 };
        let (bundle, components) = s
            .sample_spectrum(&s.fields[0], sampling, Spectrum::Polychromatic {})
            .unwrap();
        assert_eq!(components.len(), 2);
        assert_eq!(bundle.sampled_rays, 74);
        assert!((bundle.transmitted_weight() - 4.).abs() < 1e-12);
        assert!(
            s.sample_spectrum(
                &s.fields[0],
                sampling,
                Spectrum::Monochromatic { wavelength: 3000. }
            )
            .is_err()
        );
        let mono = s
            .sample_pupil(&s.fields[0], s.reference(), sampling)
            .unwrap()
            .json()
            .unwrap();
        let poly = analysis::polychromatic_spot(&s, &s.fields[0], sampling).unwrap();
        assert!(
            (mono["rms_radius"].as_f64().unwrap() - poly["rms_radius"].as_f64().unwrap()).abs()
                < 1e-12
        );
        assert_eq!(poly["failures"].as_array().unwrap().len(), 2);
        assert_eq!(poly["wavelengths"].as_array().unwrap().len(), 74);
    }
    #[test]
    fn area_analyses_integrate_nominal_pupil_despite_stored_ray_compression() {
        for mapping in [PupilMapping::RealStop, PupilMapping::ReferenceEntrancePupil] {
            for sampling in [
                PupilSampling::Grid { count: 17 },
                PupilSampling::Hexapolar { rings: 4 },
            ] {
                let mut s = planes(0.6);
                s.pupil_mapping = mapping;
                s.update().unwrap();
                let spot = s.spot_field(&s.fields[0], s.reference(), sampling).unwrap();
                assert!(spot["blocked"].as_u64().unwrap() > 0);
                let poly = analysis::polychromatic_spot(&s, &s.fields[0], sampling).unwrap();
                let energy =
                    analysis::encircled_energy(&s, &s.fields[0], s.reference(), sampling, 5)
                        .unwrap();
                let args = json!({"field_index":0,"sampling":sampling,"spectrum":{"kind":"polychromatic"}});
                let mtf = crate::compute::geometric_mtf(&s, &args).unwrap();
                s.fields[0].vignetting = [0.4, 0.2, -0.1, 0.3];
                s.update().unwrap();
                assert_eq!(
                    spot,
                    s.spot_field(&s.fields[0], s.reference(), sampling).unwrap()
                );
                assert_eq!(
                    poly,
                    analysis::polychromatic_spot(&s, &s.fields[0], sampling).unwrap()
                );
                assert_eq!(
                    energy,
                    analysis::encircled_energy(&s, &s.fields[0], s.reference(), sampling, 5)
                        .unwrap()
                );
                assert_eq!(mtf, crate::compute::geometric_mtf(&s, &args).unwrap());
            }
        }
    }
}
