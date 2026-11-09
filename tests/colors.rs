use super::*;

#[test]
fn oklab_round_trips_srgb() {
    for rgb in [
        [0, 0, 0],
        [255, 255, 255],
        [200, 120, 30],
        [12, 200, 90],
        [90, 40, 210],
    ] {
        let back = srgb(oklab(rgb));
        for a in 0..3 {
            assert!(
                (i32::from(back[a]) - i32::from(rgb[a])).abs() <= 1,
                "{rgb:?} {back:?}"
            );
        }
    }
    assert!((oklab([255, 255, 255])[0] - 1.0).abs() < 1e-3);
}

#[test]
fn median_cut_and_kmeans_find_two_groups() {
    let mut samples = Vec::new();
    for i in 0..50u8 {
        samples.push((oklab([200 + i % 5, 100 + i % 7, 20]), 1.0));
        samples.push((oklab([20, 120 + i % 6, 200 + i % 4]), 1.0));
    }
    let clusters = cluster(&samples, 2, None);
    assert_eq!(clusters.centres.len(), 2);
    assert_eq!(clusters.accents, 0);
    let orange = nearest(oklab([202, 103, 20]), &clusters.centres);
    let blue = nearest(oklab([20, 122, 201]), &clusters.centres);
    assert_ne!(orange, blue);
}

#[test]
fn the_accent_guard_keeps_a_small_dark_eye() {
    // 1000 orange cells in two shades and 6 black ones (0.6%).
    let mut samples = Vec::new();
    for i in 0..1000u32 {
        let shade = if i % 2 == 0 {
            [230, 130, 40]
        } else {
            [210, 110, 30]
        };
        samples.push((oklab(shade), 1.0));
    }
    for _ in 0..6 {
        samples.push((oklab([20, 15, 10]), 1.0));
    }
    // Two clusters spend themselves on the orange shades without a guard.
    let plain = cluster(&samples, 1, None);
    assert_eq!(plain.centres.len(), 1);
    let guarded = cluster(&samples, 1, Some(Guard::default()));
    assert_eq!(guarded.accents, 1);
    let eye = guarded.centres[1];
    assert!(distance(eye, oklab([20, 15, 10])) < 0.02, "{eye:?}");
    // Too small a share is not an accent.
    let strict = Guard {
        min_share: 0.01,
        ..Guard::default()
    };
    assert_eq!(cluster(&samples, 1, Some(strict)).accents, 0);
}
