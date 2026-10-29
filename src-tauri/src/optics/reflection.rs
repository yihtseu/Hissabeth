// SPDX-License-Identifier: GPL-3.0-or-later
// Independent geometric and Gaussian checks for centered sequential mirrors.
use super::*;

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() < tolerance,
        "{actual} vs {expected}"
    );
}

fn mirror(radius: f64, gap: f64, conic: f64) -> System {
    let mut s = System::default();
    let mut surface = Surface::sphere(radius, gap, "N-BK7_SCHOTT", 10.);
    surface.interaction = Interaction::Reflect;
    surface.conic = conic;
    s.surfaces = vec![surface];
    s.fields = vec![Field::meridional(0.)];
    s.wavelengths = vec![Wavelength::new(587.562, 1.)];
    s.reference_wavelength = 0;
    s.update().unwrap();
    s
}

#[test]
fn spherical_mirrors_match_closed_form_axis_intercepts_and_keep_physical_media() {
    for radius in [-100., 100.] {
        let mut s = mirror(radius, radius / 2., 0.);
        for medium in ["AIR", "1.500000:60.000000"] {
            s.object_material = medium.into();
            s.update().unwrap();
            let w = s.reference();
            let n = s.object_index(w).unwrap();
            let p = s.paraxial(w).unwrap();
            close(p["efl"].as_f64().unwrap(), -radius / 2., 1e-12);
            close(p["bfl"].as_f64().unwrap(), radius / 2., 1e-12);
            close(p["image_distance"].as_f64().unwrap(), radius / 2., 1e-12);
            close(p["image_index"].as_f64().unwrap(), n, 1e-12);
            assert_eq!(p["image_direction"], -1.);
            for height in [-5_f64, 2.5, 5.] {
                let ray = s.trace(0., height, 0., w).unwrap();
                let segment = ray.last();
                close(segment.n_before, n, 1e-12);
                close(segment.n_after, n, 1e-12);
                let cosine = (1. - (height / radius).powi(2)).sqrt();
                let focus = radius - radius / (2. * cosine);
                let intercept = segment.point[2]
                    - segment.point[1] * segment.direction[2] / segment.direction[1];
                close(intercept, focus, 1e-10);
                let expected = sub(
                    segment.incoming,
                    scale(segment.normal, 2. * dot(segment.incoming, segment.normal)),
                );
                close(norm(sub(segment.direction, expected)), 0., 1e-12);
            }
        }
    }
}

#[test]
fn parabola_has_common_exact_focus_constant_path_and_unaberrated_wavefront() {
    let s = mirror(-100., -50., -1.);
    let w = s.reference();
    for [x, y] in [[0., 0.], [0., 5.], [3., 4.], [-2.5, 1.]] {
        let ray = s.trace(x, y, 0., w).unwrap();
        close(norm(sub(ray.image(), [0., 0., -50.])), 0., 1e-10);
        close(ray.opl + ray.entrance_opl, 50., 1e-10);
        assert!(ray.direction[2] < 0.);
    }
    let wf = analysis::wavefront(&s, &s.fields[0], w, 17).unwrap();
    assert_eq!(wf.failed, 0);
    assert!(wf.values.iter().flatten().all(|value| value.abs() < 1e-7));
    let psf = analysis::diffraction_psf(&s, &s.fields[0], w, 32).unwrap();
    close(psf["strehl"].as_f64().unwrap(), 1., 1e-10);
    close(
        psf["pixel_size"].as_f64().unwrap(),
        w * 1e-6 * 50. / (2. * 10.),
        1e-14,
    );
    close(
        psf["intensity"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .sum(),
        1.,
        1e-12,
    );
    let longitudinal = analysis::longitudinal(&s, w, 9).unwrap();
    for point in longitudinal["points"].as_array().unwrap() {
        assert!(point["error"].is_null());
        close(point["focus"].as_f64().unwrap(), -50., 1e-9);
    }
    let astigmatism = analysis::astigmatism(&s, &s.fields[0], w).unwrap();
    close(astigmatism["sagittal_focus"].as_f64().unwrap(), -50., 1e-8);
    close(
        astigmatism["tangential_focus"].as_f64().unwrap(),
        -50.,
        1e-8,
    );
}

#[test]
fn finite_plane_mirror_returns_virtual_conjugates_without_inventing_focal_lengths() {
    let mut s = mirror(0., 10., 0.);
    s.object_distance = Some(300.);
    s.pupil.value = 2.;
    for field_type in [FieldType::ObjectHeight, FieldType::ImageHeight] {
        s.field_type = field_type;
        s.fields = vec![Field {
            x: 1.2,
            y: -2.,
            ..Field::meridional(0.)
        }];
        s.update().unwrap();
        let p = s.paraxial(s.reference()).unwrap();
        assert!(p["efl"].is_null() && p["bfl"].is_null() && p["front_focal_distance"].is_null());
        close(p["image_distance"].as_f64().unwrap(), 300., 1e-10);
        close(p["magnification"].as_f64().unwrap(), 1., 1e-12);
        assert_eq!(p["image_direction"], -1.);
        let focused = solve::autofocus(&s, s.reference()).unwrap();
        close(focused.surfaces[0].thickness, 300., 1e-9);
        for [x, y] in [[0., 0.], [0.8, 0.], [0., -0.7]] {
            let ray = focused
                .trace_field(x, y, &focused.fields[0], focused.reference())
                .unwrap();
            close(norm(sub(ray.image(), [1.2, -2., 300.])), 0., 1e-10);
            close(ray.opl + ray.entrance_opl, 0., 1e-10);
        }
        let wf = analysis::wavefront(&focused, &focused.fields[0], focused.reference(), 9).unwrap();
        assert!(wf.values.iter().flatten().all(|value| value.abs() < 1e-7));
    }
}

#[test]
fn finite_spherical_mirror_conjugates_focus_and_marginal_constraints_agree() {
    let mut s = mirror(-100., -40., 0.);
    s.object_distance = Some(300.);
    s.pupil.value = 0.02;
    s.update().unwrap();
    let p = s.paraxial(s.reference()).unwrap();
    close(p["image_distance"].as_f64().unwrap(), -60., 1e-12);
    close(p["magnification"].as_f64().unwrap(), -0.2, 1e-12);
    let focused = solve::autofocus(&s, s.reference()).unwrap();
    close(focused.surfaces[0].thickness, -60., 1e-9);
    for zone in [0., 0.5, -1.] {
        let (solved, _) =
            solve::apply(&s, 0, solve::GapSolve::MarginalHeight { value: 0., zone }).unwrap();
        close(solved.surfaces[0].thickness, -60., 1e-5);
        let ray = solved.trace(0., 0.005, 0., s.reference()).unwrap();
        close(ray.image()[1], 0., 1e-9);
    }
}

#[test]
fn two_mirror_telescope_matches_hand_matrix_and_positive_real_path() {
    let mut s = mirror(-200., -80., 0.);
    let mut second = Surface::sphere(-50., 100., "N-BK7_SCHOTT", 10.);
    second.interaction = Interaction::Reflect;
    s.surfaces.push(second);
    s.update().unwrap();
    let p = s.paraxial(s.reference()).unwrap();
    for (value, expected) in p["matrix"]
        .as_array()
        .unwrap()
        .iter()
        .zip([0.2, 80., -0.002, 4.2])
    {
        close(value.as_f64().unwrap(), expected, 1e-12);
    }
    close(p["efl"].as_f64().unwrap(), 500., 1e-9);
    close(p["bfl"].as_f64().unwrap(), 100., 1e-9);
    assert_eq!(p["image_direction"], 1.);
    assert_eq!(p["image_index"], 1.);
    let ray = s.trace(0., 0.0001, 0., s.reference()).unwrap();
    close(ray.image()[2], 20., 1e-12);
    close(ray.image()[1], 0., 1e-10);
    assert!(ray.segments[0].direction[2] < 0. && ray.segments[1].direction[2] > 0.);
    let chief = s.trace(0., 0., 0., s.reference()).unwrap();
    close(chief.opl + chief.entrance_opl, 180., 1e-10);
}

#[test]
fn mangin_round_trip_refracts_with_signed_matrices_and_physical_glass_indices() {
    let mut s = mirror(-100., -8., 0.);
    let mirror = s.surfaces.remove(0);
    s.surfaces = vec![
        Surface::sphere(60., 8., "N-BK7_SCHOTT", 10.),
        mirror,
        Surface::sphere(60., -70., "AIR", 10.),
    ];
    s.pupil.value = 0.001;
    s.update().unwrap();
    let n = s.material_index("N-BK7_SCHOTT", s.reference()).unwrap();
    // Direct products of elementary 2x2 arrays, independent of Matrix methods.
    let mut expected = [[1., 0.], [0., 1.]];
    for element in [
        [[1., 0.], [-(n - 1.) / 60., 1.]],
        [[1., 8. / n], [0., 1.]],
        [[1., 0.], [-2. * n / 100., 1.]],
        [[1., 8. / n], [0., 1.]],
        [[1., 0.], [-(n - 1.) / 60., 1.]],
    ] {
        expected = std::array::from_fn(|i| {
            std::array::from_fn(|j| (0..2).map(|k| element[i][k] * expected[k][j]).sum())
        });
    }
    let (m, image_index) = s.matrix(s.reference(), 0, 3).unwrap();
    for (actual, other) in [m.a, m.b, m.c, m.d]
        .into_iter()
        .zip(expected.into_iter().flatten())
    {
        close(actual, other, 1e-12);
    }
    close(image_index, -1., 1e-12);
    let (post, _) = s.matrix(s.reference(), 2, 3).unwrap();
    close(post.c, -(n - 1.) / 60., 1e-12);
    let height = 0.0001;
    let ray = s.trace(0., height, 0., s.reference()).unwrap();
    close(ray.segments[1].n_before, n, 1e-12);
    close(ray.segments[1].n_after, n, 1e-12);
    close(ray.last().n_before, n, 1e-12);
    close(ray.last().n_after, 1., 1e-12);
    close(ray.last().point[1], m.a * height, 1e-11);
    close(
        -ray.last().direction[1] / ray.last().direction[2],
        m.c * height,
        1e-11,
    );
    let chief = s.trace(0., 0., 0., s.reference()).unwrap();
    close(chief.opl + chief.entrance_opl, 16. * n + 70., 1e-10);
}

#[test]
fn reflected_stop_aiming_and_dummy_medium_preserve_both_pupil_mappings() {
    let mut s = mirror(-100., -20., -1.);
    let mut stop = Surface::sphere(0., -30., "N-BK7_SCHOTT", 10.);
    stop.interaction = Interaction::Dummy;
    s.surfaces.push(stop);
    s.stop_index = 1;
    s.fields = vec![Field {
        x: 0.2,
        y: -0.3,
        ..Field::meridional(0.)
    }];
    for mapping in [PupilMapping::RealStop, PupilMapping::ReferenceEntrancePupil] {
        s.pupil_mapping = mapping;
        s.update().unwrap();
        let p = s.paraxial(s.reference()).unwrap();
        close(
            p["entrance_pupil_distance"].as_f64().unwrap(),
            100. / 3.,
            1e-10,
        );
        close(p["exit_pupil_distance"].as_f64().unwrap(), 0., 1e-12);
        let ep = s.pupil_matrix(s.reference()).unwrap();
        for [x, y] in [[0., 0.], [1., -2.]] {
            let ray = s.trace_field(x, y, &s.fields[0], s.reference()).unwrap();
            close(ray.last().n_after, 1., 1e-12);
            if mapping == PupilMapping::RealStop || (x == 0. && y == 0.) {
                close(ray.last().point[0], ep.a * x, 1e-8);
                close(ray.last().point[1], ep.a * y, 1e-8);
            }
        }
    }
}

#[test]
fn reflection_na_pupils_use_physical_index_and_signed_edge_targets_resolve() {
    let mut s = mirror(-100., -50., -1.);
    s.object_material = "1.500000:60.000000".into();
    s.pupil = Pupil {
        kind: PupilType::ImageNa,
        value: 1.2,
    };
    s.update().unwrap();
    let p = s.paraxial(s.reference()).unwrap();
    close(p["image_na"].as_f64().unwrap(), 1.2, 1e-12);
    s.pupil = Pupil {
        kind: PupilType::FNumber,
        value: 5.,
    };
    s.update().unwrap();
    close(
        s.paraxial(s.reference()).unwrap()["f_number"]
            .as_f64()
            .unwrap(),
        5.,
        1e-12,
    );
    let definition = solve::GapSolve::EdgeThickness {
        height: 3.,
        value: -60.,
    };
    let (solved, _) = solve::apply(&s, 0, definition).unwrap();
    close(solved.surfaces[0].thickness, -60. - 9. / 200., 1e-10);
    let mut exportable = solved;
    exportable.surfaces[0].gap_solve = solve::GapSolve::Fixed {};
    exportable.pupil_mapping = PupilMapping::ReferenceEntrancePupil;
    exportable.object_distance = Some(300.);
    exportable.temperature = 25.;
    assert!(
        project::export(&exportable)
            .unwrap_err()
            .contains("refractive")
    );
}

#[test]
fn reflected_image_aperture_limits_vignetting_and_unclipped_envelope() {
    let mut s = mirror(-100., -25., -1.);
    s.image_surface.aperture = 1.25;
    s.image_surface.clear_aperture = true;
    s.update().unwrap();
    let report = aperture::semi_diameters(&s).unwrap();
    assert!(report.image.semi_diameter > 2.5);
    let (next, report) = aperture::update_vignetting(&s).unwrap();
    for factor in next.fields[0].vignetting {
        assert!(factor > 0.49 && factor < 0.51);
    }
    for boundary in &report.fields[0].boundaries {
        assert_eq!(boundary.limiting_endpoint, Some("image"));
    }
    let r = next.pupil_radius(next.reference()).unwrap();
    for [x, y] in [[r, 0.], [-r, 0.], [0., r], [0., -r]] {
        let ray = next
            .trace_field(x, y, &next.fields[0], next.reference())
            .unwrap();
        assert!(ray.image()[0].hypot(ray.image()[1]) <= 1.25 + 1e-9);
    }
}
