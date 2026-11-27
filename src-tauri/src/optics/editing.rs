// SPDX-License-Identifier: GPL-3.0-or-later
// Draft topology edits share one index transform across GUI and MCP.
use super::{Interaction, Surface, System, solve::GapSolve};
use serde::{Deserialize, Serialize};

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Insert {
        #[schemars(range(min = 0, max = 128))]
        before: usize,
        #[schemars(range(min = 1, max = 128))]
        count: usize,
    },
    Remove {
        #[schemars(range(min = 0, max = 127))]
        first: usize,
        #[schemars(range(min = 1, max = 128))]
        count: usize,
    },
}

#[derive(schemars::JsonSchema, Debug, Serialize)]
pub struct Edit {
    pub system: System,
    pub old_to_new: Vec<Option<usize>>,
    pub inserted: Vec<usize>,
    pub stop_reassigned: bool,
}

pub fn apply(source: &System, operation: Operation) -> Result<Edit, String> {
    let n = source.surfaces.len();
    if !(1..=128).contains(&n) || source.stop_index >= n {
        return Err("Surface rows and stop must identify an existing prescription".into());
    }
    let (removed, inserted) = match operation {
        Operation::Insert { before, count } => {
            if before > n || count == 0 || count > 128 - n {
                return Err("Insertion must produce 1–128 rows and identify a position before a surface or Image".into());
            }
            (None, (before..before + count).collect::<Vec<_>>())
        }
        Operation::Remove { first, count } => {
            if first >= n || count == 0 || count >= n || count > n - first {
                return Err(
                    "Removal must identify existing contiguous rows and leave at least one surface"
                        .into(),
                );
            }
            (Some(first..first + count), vec![])
        }
    };
    let retained = |i: usize| !removed.as_ref().is_some_and(|r| r.contains(&i));
    // A removed row's own definition disappears with its owned following gap.
    for (owner, surface) in source
        .surfaces
        .iter()
        .enumerate()
        .filter(|(i, _)| retained(*i))
    {
        surface
            .gap_solve
            .validate(source, owner)
            .map_err(|_| format!("Invalid solve definition on surface {}", owner + 1))?;
        if let Some(range) = &removed {
            let referenced = match surface.gap_solve {
                GapSolve::Pickup { source, .. } if range.contains(&source) => Some(source),
                GapSolve::OverallLength { first, last, .. }
                    if first < range.end && range.start < last =>
                {
                    Some(first.max(range.start))
                }
                _ => None,
            };
            if let Some(gap) = referenced {
                return Err(format!(
                    "Cannot remove gap {}: the solve on surface {} refers to it. Change that solve or remove its row in the same operation.",
                    gap + 1,
                    owner + 1
                ));
            }
        }
    }
    let map_boundary = |i: usize| match operation {
        Operation::Insert { before, count } => i + if i >= before { count } else { 0 },
        Operation::Remove { first, count } => i - i.saturating_sub(first).min(count),
    };
    let old_to_new: Vec<_> = (0..n)
        .map(|i| retained(i).then(|| map_boundary(i)))
        .collect();
    let mut system = source.clone();
    system.surfaces = source
        .surfaces
        .iter()
        .enumerate()
        .filter(|(i, _)| retained(*i))
        .map(|(_, surface)| {
            let mut next = surface.clone();
            next.gap_solve = match surface.gap_solve {
                GapSolve::Pickup {
                    source,
                    scale,
                    offset,
                } => GapSolve::Pickup {
                    source: map_boundary(source),
                    scale,
                    offset,
                },
                GapSolve::OverallLength { first, last, value } => GapSolve::OverallLength {
                    first: map_boundary(first),
                    // Inserting exactly at the exclusive end does not extend the interval.
                    last: match operation {
                        Operation::Insert { before, count } => {
                            last + if last > before { count } else { 0 }
                        }
                        _ => map_boundary(last),
                    },
                    value,
                },
                other => other,
            };
            next
        })
        .collect();
    if let Operation::Insert { before, count } = operation {
        let material = source.surfaces[..before]
            .iter()
            .rev()
            .find(|s| s.interaction == Interaction::Refract)
            .map(|s| s.material.as_str())
            .unwrap_or(&source.object_material);
        let mut row = Surface::sphere(0., 0., material, 1.);
        row.interaction = Interaction::Dummy;
        row.clear_aperture = false;
        system
            .surfaces
            .splice(before..before, std::iter::repeat_n(row, count));
    }
    let stop_reassigned = !retained(source.stop_index);
    system.stop_index = old_to_new[source.stop_index].unwrap_or_else(|| {
        removed
            .as_ref()
            .unwrap()
            .start
            .min(system.surfaces.len() - 1)
    });
    Ok(Edit {
        system,
        old_to_new,
        inserted,
        stop_reassigned,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optics::*;
    use serde_json::to_value;

    fn draft(n: usize) -> System {
        let mut s = System::default();
        s.surfaces = (0..n)
            .map(|i| {
                let mut row = Surface::sphere(0., (i + 1) as f64, "AIR", 4.);
                row.label = format!("physical-{i}");
                row
            })
            .collect();
        s.stop_index = n - 1;
        s
    }
    #[test]
    fn surface_edit_insertion_maps_physical_rows_and_half_open_constraints() {
        let mut s = draft(5);
        s.surfaces[1].gap_solve = GapSolve::Pickup {
            source: 4,
            scale: 2.,
            offset: 3.,
        };
        s.surfaces[3].gap_solve = GapSolve::OverallLength {
            first: 2,
            last: 5,
            value: 50.,
        };
        let original = to_value(&s).unwrap();
        for before in 0..=5 {
            let e = apply(&s, Operation::Insert { before, count: 2 }).unwrap();
            assert_eq!(e.inserted, vec![before, before + 1]);
            for old in 0..5 {
                let new = old + if old >= before { 2 } else { 0 };
                assert_eq!(e.old_to_new[old], Some(new));
                assert_eq!(e.system.surfaces[new].label, s.surfaces[old].label);
                e.system.surfaces[new]
                    .gap_solve
                    .validate(&e.system, new)
                    .unwrap();
            }
            let p = e.old_to_new[1].unwrap();
            assert!(
                matches!(e.system.surfaces[p].gap_solve,GapSolve::Pickup {source,..} if source==e.old_to_new[4].unwrap())
            );
            let o = e.old_to_new[3].unwrap();
            assert!(
                matches!(e.system.surfaces[o].gap_solve,GapSolve::OverallLength {first,last,..} if first==2+(if 2>=before {2}else{0}) && last==5+(if 5>before {2}else{0}))
            );
            assert_eq!(e.system.stop_index, e.old_to_new[4].unwrap());
            assert!(!e.stop_reassigned);
        }
        assert_eq!(to_value(&s).unwrap(), original);
    }
    #[test]
    fn surface_edit_removal_guards_retained_dependencies_and_allows_clusters() {
        let mut s = draft(5);
        s.surfaces[2].gap_solve = GapSolve::Pickup {
            source: 1,
            scale: 1.,
            offset: 0.,
        };
        assert!(
            apply(&s, Operation::Remove { first: 1, count: 1 })
                .unwrap_err()
                .contains("surface 3")
        );
        let e = apply(&s, Operation::Remove { first: 1, count: 2 }).unwrap();
        assert_eq!(e.old_to_new, vec![Some(0), None, None, Some(1), Some(2)]);
        s.surfaces[4].gap_solve = GapSolve::OverallLength {
            first: 3,
            last: 5,
            value: 8.,
        };
        assert!(
            apply(&s, Operation::Remove { first: 3, count: 1 })
                .unwrap_err()
                .contains("gap 4")
        );
        let e = apply(&s, Operation::Remove { first: 1, count: 2 }).unwrap();
        assert!(matches!(
            e.system.surfaces[2].gap_solve,
            GapSolve::OverallLength {
                first: 1,
                last: 3,
                ..
            }
        ));
        // A malformed definition is removable along with its owner.
        s.surfaces[2].gap_solve = GapSolve::Pickup {
            source: 999,
            scale: 1.,
            offset: 0.,
        };
        apply(&s, Operation::Remove { first: 1, count: 2 }).unwrap();
        assert!(
            apply(
                &s,
                Operation::Insert {
                    before: 1,
                    count: 1
                }
            )
            .is_err()
        );
    }
    #[test]
    fn surface_edit_stop_boundaries_limits_and_optically_unresolved_drafts() {
        let mut s = draft(5);
        s.stop_index = 2;
        let e = apply(&s, Operation::Remove { first: 1, count: 2 }).unwrap();
        assert!(e.stop_reassigned);
        assert_eq!(e.system.stop_index, 1);
        let e = apply(&s, Operation::Remove { first: 2, count: 3 }).unwrap();
        assert!(e.stop_reassigned);
        assert_eq!(e.system.stop_index, 1);
        for op in [
            Operation::Insert {
                before: 6,
                count: 1,
            },
            Operation::Insert {
                before: 0,
                count: 0,
            },
            Operation::Insert {
                before: 0,
                count: usize::MAX,
            },
            Operation::Remove { first: 0, count: 5 },
            Operation::Remove { first: 4, count: 2 },
            Operation::Remove {
                first: usize::MAX,
                count: 1,
            },
        ] {
            assert!(apply(&s, op).is_err());
        }
        let full = apply(
            &s,
            Operation::Insert {
                before: 5,
                count: 123,
            },
        )
        .unwrap()
        .system;
        assert_eq!(full.surfaces.len(), 128);
        assert!(
            apply(
                &full,
                Operation::Insert {
                    before: 128,
                    count: 1
                }
            )
            .is_err()
        );
        s.surfaces[0].material = "unfinished glass".into();
        s.surfaces[0].aperture = -2.;
        let e = apply(
            &s,
            Operation::Insert {
                before: 1,
                count: 1,
            },
        )
        .unwrap();
        assert_eq!(e.system.surfaces[0].aperture, -2.);
        assert_eq!(e.system.surfaces[1].material, "unfinished glass");
        assert!(e.system.validate().is_err());
        let mut bad = s.clone();
        bad.stop_index = 9;
        assert!(apply(&bad, Operation::Remove { first: 1, count: 1 }).is_err());
    }
    #[test]
    fn surface_edit_inserted_dummy_keeps_incident_medium_through_reflection() {
        let mut s = draft(4);
        s.object_material = "FUSED_SILICA".into();
        s.surfaces[0].material = "N-BK7_SCHOTT".into();
        s.surfaces[1].interaction = Interaction::Reflect;
        s.surfaces[1].material = "unused mirror".into();
        s.surfaces[2].interaction = Interaction::Dummy;
        s.surfaces[2].material = "unused dummy".into();
        for (before, medium) in [
            (0, "FUSED_SILICA"),
            (1, "N-BK7_SCHOTT"),
            (2, "N-BK7_SCHOTT"),
            (3, "N-BK7_SCHOTT"),
            (4, "AIR"),
        ] {
            let e = apply(&s, Operation::Insert { before, count: 2 }).unwrap();
            for i in e.inserted {
                let row = &e.system.surfaces[i];
                assert_eq!(row.material, medium);
                assert_eq!(row.thickness, 0.);
                assert_eq!(row.radius, 0.);
                assert_eq!(row.interaction, Interaction::Dummy);
                assert!(!row.clear_aperture);
                assert!(row.gap_solve.is_fixed());
                assert!(row.even.is_empty() && row.odd.is_empty());
            }
        }
    }
    #[test]
    fn surface_edit_zero_gap_dummy_preserves_real_rays_matrix_and_total_path() {
        let mut finite = System::default();
        finite.object_distance = Some(160.);
        finite.fields = vec![Field {
            x: 1.,
            y: 2.,
            ..Field::meridional(0.)
        }];
        finite.object_surface.radius = 500.;
        finite.image_surface.radius = -300.;
        let mut mirror = draft(1);
        mirror.surfaces[0].radius = -100.;
        mirror.surfaces[0].thickness = -50.;
        mirror.surfaces[0].interaction = Interaction::Reflect;
        mirror.fields = vec![Field::meridional(0.)];
        for mut s in [System::default(), finite, mirror] {
            for mapping in [PupilMapping::ReferenceEntrancePupil, PupilMapping::RealStop] {
                s.pupil_mapping = mapping;
                s.update().unwrap();
                let w = s.reference();
                let (mut m, n) = s.matrix(w, 0, s.surfaces.len()).unwrap();
                m.transfer(s.surfaces.last().unwrap().thickness, n);
                for before in 0..=s.surfaces.len() {
                    let mut edited = apply(&s, Operation::Insert { before, count: 2 })
                        .unwrap()
                        .system;
                    edited.update().unwrap();
                    let (mut em, en) = edited.matrix(w, 0, edited.surfaces.len()).unwrap();
                    em.transfer(edited.surfaces.last().unwrap().thickness, en);
                    assert_eq!([m.a, m.b, m.c, m.d], [em.a, em.b, em.c, em.d]);
                    assert_eq!(n, en);
                    for [x, y] in [[0., 0.], [0.2, 0.8], [-0.5, 0.3]] {
                        let a = s.trace_field(x, y, &s.fields[0], w).unwrap();
                        let b = edited.trace_field(x, y, &s.fields[0], w).unwrap();
                        for k in 0..3 {
                            assert!((a.image()[k] - b.image()[k]).abs() < 1e-8);
                            assert!((a.direction[k] - b.direction[k]).abs() < 1e-10);
                        }
                        assert!((a.opl + a.entrance_opl - b.opl - b.entrance_opl).abs() < 1e-8);
                    }
                }
            }
        }
    }
}
