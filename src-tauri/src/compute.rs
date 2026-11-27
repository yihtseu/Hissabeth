use crate::optics::analysis::Otf;
use serde_json::Value;
#[cfg(test)]
use serde_json::json;
pub fn geometric_mtf(system: &crate::optics::System, args: &Value) -> Result<Value, String> {
    use crate::optics::{PupilSampling, Spectrum};
    let spectrum: Spectrum =
        serde_json::from_value(args["spectrum"].clone()).map_err(|e| e.to_string())?;
    let sampling: PupilSampling =
        serde_json::from_value(args["sampling"].clone()).map_err(|e| e.to_string())?;
    let field = crate::service::field_argument(system, args)?;
    serde_json::to_value(geometric_mtf_field(
        system,
        &field,
        sampling,
        spectrum,
        args["axis"].as_str().unwrap_or("y"),
        args["max_frequency"].as_f64().unwrap_or(100.),
    )?)
    .map_err(|e| e.to_string())
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct GeometricMtf {
    spectrum: crate::optics::Spectrum,
    sampling: crate::optics::PupilSampling,
    axis: &'static str,
    frequencies: Vec<f64>,
    mtf: Vec<f64>,
    otf: Otf,
    origin: [f64; 2],
    components: Vec<crate::optics::SpectralComponent>,
    transmitted_weight: f64,
    normalization: &'static str,
    units: &'static str,
    backend: &'static str,
    rays: usize,
}

pub(crate) fn geometric_mtf_field(
    system: &crate::optics::System,
    field: &crate::optics::Field,
    sampling: crate::optics::PupilSampling,
    spectrum: crate::optics::Spectrum,
    axis: &str,
    max: f64,
) -> Result<GeometricMtf, String> {
    let (bundle, components) = system.sample_spectrum(field, sampling, spectrum)?;
    let axis = match axis {
        "x" => 0,
        "y" => 1,
        _ => return Err("axis must be x or y".into()),
    };
    let origin = bundle.centroid()?;
    let total = bundle.transmitted_weight();
    if !max.is_finite() || max <= 0. || max > 10000. {
        return Err("max_frequency must be in (0,10000] cycles/mm".into());
    }
    let frequencies: Vec<_> = (0..65).map(|i| max * i as f64 / 64.).collect();
    let mut real = vec![];
    let mut imaginary = vec![];
    let mut values = vec![];
    for frequency in &frequencies {
        let (re, im) = bundle.points.iter().zip(&bundle.weights).fold(
            (0., 0.),
            |(re, im), (point, weight)| {
                let angle = std::f64::consts::TAU * frequency * (point[axis] - origin[axis]);
                let (sin, cos) = angle.sin_cos();
                (re + weight * cos / total, im - weight * sin / total)
            },
        );
        real.push(re);
        imaginary.push(im);
        values.push(re.hypot(im));
    }
    Ok(GeometricMtf {
        spectrum,
        sampling,
        axis: if axis == 0 { "x" } else { "y" },
        frequencies,
        mtf: values,
        otf: Otf { real, imaginary },
        origin,
        components,
        transmitted_weight: total,
        normalization: "incident spectral weights times sampled pupil transmission; normalized at DC",
        units: "cycles/mm",
        backend: "cpu",
        rays: bundle.points.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mtf_normalized_at_dc() {
        let r = geometric_mtf(&crate::optics::System::default(), &json!({"sampling":{"kind":"grid","count":9},"spectrum":{"kind":"monochromatic","wavelength":587.5618}})).unwrap();
        assert!((r["mtf"][0].as_f64().unwrap() - 1.).abs() < 1e-12);
        assert!(
            r["mtf"]
                .as_array()
                .unwrap()
                .iter()
                .all(|v| v.as_f64().unwrap() <= 1.00000001)
        );
    }
}

#[cfg(test)]
mod spectral_tests {
    use super::*;
    use crate::optics::{Field, System, Wavelength};
    fn chromatic_system() -> System {
        let mut s = System {
            stop_index: 1,
            ..Default::default()
        };
        s.pupil.value = 0.00001;
        s.fields = vec![Field::meridional(5.)];
        s.wavelengths = vec![Wavelength::new(486.1327, 1.), Wavelength::new(656.2725, 1.)];
        s.reference_wavelength = 0;
        s.update().unwrap();
        s
    }
    #[test]
    fn polychromatic_geometric_otf_keeps_lateral_color_and_weighted_phase() {
        let mut s = chromatic_system();
        let a = s
            .trace_field(0., 0., &s.fields[0], s.wavelengths[0].value)
            .unwrap()
            .image();
        let b = s
            .trace_field(0., 0., &s.fields[0], s.wavelengths[1].value)
            .unwrap()
            .image();
        let max = 1. / (2. * (a[1] - b[1]).abs());
        assert!(max < 10000.);
        let args = json!({"field_index":0,"sampling":{"kind":"hexapolar","rings":2},"spectrum":{"kind":"polychromatic"},"axis":"y","max_frequency":max});
        let poly = geometric_mtf(&s, &args).unwrap();
        assert!(poly["mtf"][64].as_f64().unwrap() < 1e-4, "{poly}");
        for wave in &s.wavelengths {
            let mut mono = args.clone();
            mono["spectrum"] = json!({"kind":"monochromatic","wavelength":wave.value});
            assert!(
                geometric_mtf(&s, &mono).unwrap()["mtf"][64]
                    .as_f64()
                    .unwrap()
                    > 0.9999
            );
        }
        s.wavelengths[1].weight = 3.;
        let weighted = geometric_mtf(&s, &args).unwrap();
        assert!((weighted["mtf"][64].as_f64().unwrap() - 0.5).abs() < 1e-4);
        let total = weighted["transmitted_weight"].as_f64().unwrap();
        for wave in &mut s.wavelengths {
            wave.weight *= 7.;
        }
        let scaled = geometric_mtf(&s, &args).unwrap();
        assert!(
            (scaled["mtf"][64].as_f64().unwrap() - weighted["mtf"][64].as_f64().unwrap()).abs()
                < 1e-12
        );
        assert!((scaled["transmitted_weight"].as_f64().unwrap() - 7. * total).abs() < 1e-10);
    }
    #[test]
    fn fully_blocked_spectral_component_has_zero_weight_without_recentering() {
        let mut s = chromatic_system();
        let a = s
            .trace_field(0., 0., &s.fields[0], s.wavelengths[0].value)
            .unwrap()
            .segments[0]
            .point;
        let b = s
            .trace_field(0., 0., &s.fields[0], s.wavelengths[1].value)
            .unwrap()
            .segments[0]
            .point;
        s.surfaces[0].aperture = (a[1].abs() + b[1].abs()) / 2.;
        s.update().unwrap();
        let args = json!({"field_index":0,"sampling":{"kind":"grid","count":5},"spectrum":{"kind":"polychromatic"},"axis":"y"});
        let result = geometric_mtf(&s, &args).unwrap();
        let components = result["components"].as_array().unwrap();
        assert_eq!(components.len(), 2);
        assert_eq!(
            components
                .iter()
                .filter(|c| c["transmitted_samples"] == 0)
                .count(),
            1
        );
        assert!((result["transmitted_weight"].as_f64().unwrap() - 1.).abs() < 1e-12);
        assert!((result["mtf"][0].as_f64().unwrap() - 1.).abs() < 1e-12);
    }
}
