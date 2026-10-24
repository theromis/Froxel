use super::shade::{ggx_sample_vndf, nishita_sky, sky};
use super::world::{build_world, march};
use super::*;
use crate::state::{Material, MaterialType, Session};

fn voxel(session: &mut Session, cell: (i32, i32, i32), slot: u8) {
    session.objects[0].cells.insert(cell, slot);
}

#[test]
fn a_ray_hits_the_voxel_in_front_of_it() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    voxel(&mut session, (0, 0, 0), 1);
    session.target = Vec3::new(0.5, 0.5, 0.5);
    session.distance = 6.0;
    session.yaw = 0.0;
    session.pitch = 0.0;
    let image = Image::new(1, 1);
    let world = build_world(&session);
    let eye = Vec3::new(0.5, -4.0, 0.5);
    let hit = march(&world, eye, Vec3::Y).expect("hit");
    assert_eq!(hit.slot, 1);
    assert!(image.samples == 0);
}

#[test]
fn an_emissive_voxel_lights_the_pixel() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    session.stochastic_gi = false;
    session.sun_intensity = 0.0;
    session.sky_intensity = 0.0;
    session.aces = false;
    session.denoise = false;
    session.ema_floor = 0.0;
    voxel(&mut session, (0, 0, 0), 4);
    session.materials[4] = Material {
        ty: MaterialType::Emissive,
        emission: 1.0,
        emission_power: 2,
        ..Material::default()
    };
    session.palette[4] = [255, 0, 0, 255];
    session.target = Vec3::new(0.5, 0.5, 0.5);
    session.yaw = -std::f32::consts::FRAC_PI_2;
    session.pitch = 0.0;
    session.distance = 1.2;
    let mut image = Image::new(1, 1);
    trace_rows(&mut image, &session, 1);
    let px = pixels(&image, &session);
    assert!(px[0] > px[1] && px[0] > 20, "rgba {px:?}");
}

#[test]
fn an_emissive_neighbor_lights_a_diffuse_face() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    session.sun_intensity = 0.0;
    session.sky_intensity = 0.0;
    session.aces = false;
    session.denoise = false;
    session.ema_floor = 0.0;
    session.stochastic_gi = true;
    voxel(&mut session, (0, 0, 0), 4);
    voxel(&mut session, (2, 2, 0), 1);
    session.materials[4] = Material {
        ty: MaterialType::Emissive,
        emission: 1.0,
        emission_power: 2,
        ..Material::default()
    };
    session.palette[4] = [255, 0, 0, 255];
    session.palette[1] = [255, 255, 255, 255];
    session.target = Vec3::new(2.5, 2.5, 0.5);
    session.yaw = -std::f32::consts::FRAC_PI_2;
    session.pitch = 0.0;
    session.distance = 1.2;
    let mut image = Image::new(1, 1);
    trace_rows(&mut image, &session, 1);
    let px = pixels(&image, &session);
    assert!(px[0] > px[1] && px[0] > 20, "rgba {px:?}");
}

#[test]
fn noon_nishita_is_blue_and_a_downward_ray_is_dark() {
    let white = [1.0, 1.0, 1.0];
    let noon = nishita_sky(Vec3::Z, Vec3::Z, white);
    let down = nishita_sky(-Vec3::Z, Vec3::Z, white);
    let sunset = nishita_sky(Vec3::Z, Vec3::X, white);
    assert!(noon.iter().all(|c| c.is_finite() && *c > 0.0), "{noon:?}");
    assert!(noon[2] > noon[0], "noon {noon:?}");
    assert!(noon[2] > down[2] * 4.0 + 1.0e-4, "down {down:?}");
    assert!(noon[2] > sunset[2], "sunset {sunset:?}");
}

#[test]
fn a_smooth_ggx_facet_is_the_mirror_and_rough_samples_stay_above() {
    let normal = Vec3::Z;
    let dir = Vec3::new(0.2, -0.1, -1.0).normalize();
    let mut rng = super::Rng(1);
    let facet = ggx_sample_vndf(normal, -dir, 0.0, &mut rng);
    assert!(facet.dot(normal) > 0.999, "{facet:?}");
    let mirror = dir - normal * (2.0 * dir.dot(normal));
    let wi = dir - facet * (2.0 * dir.dot(facet));
    assert!(wi.normalize().dot(mirror.normalize()) > 0.999);
    for _ in 0..24 {
        let facet = ggx_sample_vndf(normal, -dir, 0.4, &mut rng);
        let wi = (dir - facet * (2.0 * dir.dot(facet))).normalize();
        assert!(wi.dot(normal) > 0.0, "{wi:?}");
    }
}

#[test]
fn a_metal_with_zero_metallic_matches_diffuse() {
    let diffuse = neighbor_scene();
    let mut metal = neighbor_scene();
    metal.materials[1].ty = MaterialType::Metal;
    metal.materials[1].metallic = 0.0;
    let mut left = Image::new(1, 1);
    let mut right = Image::new(1, 1);
    trace_rows(&mut left, &diffuse, 1);
    trace_rows(&mut right, &metal, 1);
    assert_eq!(pixels(&left, &diffuse), pixels(&right, &metal));
}

fn neighbor_scene() -> Session {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    session.sun_intensity = 0.0;
    session.sky_intensity = 0.0;
    session.aces = false;
    session.denoise = false;
    session.ema_floor = 0.0;
    session.stochastic_gi = true;
    voxel(&mut session, (0, 0, 0), 4);
    voxel(&mut session, (2, 2, 0), 1);
    session.materials[4] = Material {
        ty: MaterialType::Emissive,
        emission: 1.0,
        emission_power: 2,
        ..Material::default()
    };
    session.palette[4] = [255, 0, 0, 255];
    session.palette[1] = [255, 255, 255, 255];
    session.target = Vec3::new(2.5, 2.5, 0.5);
    session.yaw = -std::f32::consts::FRAC_PI_2;
    session.pitch = 0.0;
    session.distance = 1.2;
    session
}

#[test]
fn a_pending_render_is_written_only_at_the_sample_cap() {
    let mut session = Session::new();
    session.max_samples = 4;
    session.render_save_path = Some(std::path::PathBuf::from("later.png"));
    assert!(session.take_finished_render(3).is_none());
    assert!(session.render_save_path.is_some());
    let path = session.take_finished_render(4).unwrap();
    assert_eq!(path, std::path::PathBuf::from("later.png"));
    assert!(session.render_save_path.is_none());
}

#[test]
fn save_render_writes_a_png() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    voxel(&mut session, (0, 0, 0), 1);
    session.ema_floor = 0.0;
    let mut image = Image::new(2, 2);
    trace_rows(&mut image, &session, 2);
    let path = std::env::temp_dir().join("froxel-render.png");
    write_png(&path, &image, &session).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
    let _ = std::fs::remove_file(path);
}

fn atmospheric_session() -> Session {
    let mut session = Session::new();
    session.atmospheric_sky = true;
    session
}

fn mean(color: [f32; 3]) -> f32 {
    (color[0] + color[1] + color[2]) / 3.0
}

#[test]
fn the_atmospheric_sky_is_lit_with_the_sun_off_and_ignores_the_disc() {
    let session = atmospheric_session();
    assert_eq!(session.sun_intensity, 0.0);
    let shade = Shade::from_session(&session);
    let zenith = sky(&shade, Vec3::Z, false);
    let horizon = sky(&shade, Vec3::new(-1.0, 0.0, 0.01), false);
    assert!(
        zenith[2] > 0.08 && zenith[2] > zenith[0],
        "zenith {zenith:?}"
    );
    assert!(
        mean(horizon) > 0.3 && mean(horizon) < 2.0,
        "horizon {horizon:?}"
    );
    let mut small = atmospheric_session();
    small.sun_disc = 1.0;
    let small = sky(&Shade::from_session(&small), Vec3::Z, false);
    assert_eq!(zenith, small);
}

#[test]
fn the_sky_slider_scales_the_atmospheric_sky() {
    let mut session = atmospheric_session();
    let one = sky(&Shade::from_session(&session), Vec3::Z, false);
    session.sky_intensity = 2.0;
    let two = sky(&Shade::from_session(&session), Vec3::Z, false);
    for (a, b) in one.iter().zip(two) {
        assert!((a * 2.0 - b).abs() < 1.0e-5, "{one:?} {two:?}");
    }
}

#[test]
fn below_the_horizon_the_atmospheric_sky_is_ground_not_black() {
    let session = atmospheric_session();
    let shade = Shade::from_session(&session);
    let horizon = sky(&shade, Vec3::new(1.0, 0.0, 0.0), false);
    let just_below = sky(&shade, Vec3::new(1.0, 0.0, -0.001), false);
    let nadir = sky(&shade, -Vec3::Z, false);
    for (a, b) in horizon.iter().zip(just_below) {
        assert!((a - b).abs() < 0.02, "{horizon:?} {just_below:?}");
    }
    assert!(nadir.iter().all(|c| *c > 0.01), "nadir {nadir:?}");
    assert!(nadir[0] > nadir[2], "ground is warm {nadir:?}");
    assert!(mean(nadir) < mean(horizon), "{nadir:?} {horizon:?}");
}

#[test]
fn the_atmospheric_sky_shows_the_sun_disc() {
    let mut session = atmospheric_session();
    session.sun_intensity = 5.0;
    let shade = Shade::from_session(&session);
    let toward = sky(&shade, session.sun_dir(), true);
    let without = sky(&shade, session.sun_dir(), false);
    assert!(mean(toward) > mean(without) + 4.0, "{toward:?} {without:?}");
}

#[test]
fn a_pending_render_is_cancelled_when_the_frame_starts_over() {
    let mut session = Session::new();
    session.max_samples = 8;
    session.render_save_path = Some(std::path::PathBuf::from("later.png"));
    assert!(session.take_finished_render(5).is_none());
    assert!(session.take_finished_render(2).is_none());
    assert!(session.render_save_path.is_none());
    assert!(session.status.contains("cancelled"), "{}", session.status);
    assert!(session.take_finished_render(8).is_none());
}

#[test]
fn save_render_outside_render_mode_queues_nothing() {
    let mut session = Session::new();
    session.trace_on_gpu = true;
    session.save_render();
    assert!(session.render_save_path.is_none());
}

fn md5(message: &[u8]) -> String {
    fn f(x: u32, y: u32, z: u32) -> u32 {
        (x & y) | (!x & z)
    }
    fn g(x: u32, y: u32, z: u32) -> u32 {
        (x & z) | (y & !z)
    }
    fn h(x: u32, y: u32, z: u32) -> u32 {
        x ^ y ^ z
    }
    fn i(x: u32, y: u32, z: u32) -> u32 {
        y ^ (x | !z)
    }
    const SHIFT: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let mut table = [0u32; 64];
    for index in 0..64 {
        table[index] = (((index as f64 + 1.0).sin().abs()) * 4294967296.0) as u32;
    }
    let bit_len = (message.len() as u64).wrapping_mul(8);
    let mut padded = message.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_le_bytes());
    let mut a0 = 0x67452301u32;
    let mut b0 = 0xefcdab89u32;
    let mut c0 = 0x98badcfeu32;
    let mut d0 = 0x10325476u32;
    for chunk in padded.chunks_exact(64) {
        let mut words = [0u32; 16];
        for (index, word) in words.iter_mut().enumerate() {
            *word = u32::from_le_bytes(chunk[index * 4..index * 4 + 4].try_into().unwrap());
        }
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for step in 0..64 {
            let (value, word) = match step {
                0..=15 => (f(b, c, d), step),
                16..=31 => (g(b, c, d), (5 * step + 1) % 16),
                32..=47 => (h(b, c, d), (3 * step + 5) % 16),
                _ => (i(b, c, d), (7 * step) % 16),
            };
            let next = b.wrapping_add(
                a.wrapping_add(value)
                    .wrapping_add(table[step])
                    .wrapping_add(words[word])
                    .rotate_left(SHIFT[step]),
            );
            a = d;
            d = c;
            c = b;
            b = next;
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut out = String::new();
    for word in [a0, b0, c0, d0] {
        for byte in word.to_le_bytes() {
            out.push_str(&format!("{byte:02x}"));
        }
    }
    out
}

#[test]
fn md5_matches_the_empty_string_and_a() {
    assert_eq!(md5(b""), "d41d8cd98f00b204e9800998ecf8427e");
    assert_eq!(md5(b"a"), "0cc175b9c0f1b6a831c399e269772661");
}

/// A fixed CPU still. The hash is the PNG, so a tracer change has to update it.
#[test]
fn a_fixed_cpu_still_has_a_stable_png() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    voxel(&mut session, (0, 0, 0), 36);
    voxel(&mut session, (1, 0, 0), 40);
    voxel(&mut session, (0, 0, 1), 50);
    let image = crate::still::render_cpu(
        &mut session,
        &crate::still::Still {
            width: 8,
            height: 8,
            samples: 2,
            bounces: 5,
            seed: 1,
            aces: true,
            denoise: false,
            atmospheric: None,
            sky_intensity: None,
            sun_intensity: None,
            aperture: None,
            focus: None,
            glass: None,
            stochastic_gi: None,
            yaw: 30.0,
            pitch: 25.0,
            distance: 1.0,
            orthographic: None,
        },
    );
    let png = encode_png(&image, &session).expect("png");
    assert_eq!(md5(&png), "514a25acad366a621a41c108d747d528");
}

#[test]
fn yaw_zero_sits_the_camera_on_positive_x() {
    let mut session = Session::new();
    session.target = Vec3::ZERO;
    session.distance = 10.0;
    session.yaw = 0.0;
    session.pitch = 0.0;
    let eye = session.eye();
    assert!((eye.x - 10.0).abs() < 1e-4, "{eye}");
    assert!(eye.y.abs() < 1e-4 && eye.z.abs() < 1e-4, "{eye}");
    let look = (session.target - eye).normalize();
    assert!((look - Vec3::new(-1.0, 0.0, 0.0)).length() < 1e-4);
}

#[test]
fn orthographic_primary_rays_share_a_direction() {
    let eye = Vec3::new(10.0, 0.0, 0.0);
    let target = Vec3::ZERO;
    let mut left = super::Rng(1);
    let mut right = super::Rng(2);
    let (origin_a, dir_a) = super::ortho_primary(8, 8, 1, 1, eye, target, &mut left);
    let (origin_b, dir_b) = super::ortho_primary(8, 8, 6, 4, eye, target, &mut right);
    assert!((dir_a - dir_b).length() < 1e-5);
    assert!((dir_a - Vec3::new(-1.0, 0.0, 0.0)).length() < 1e-4);
    assert!((origin_a - origin_b).length() > 0.1);
}

#[test]
fn show_bones_paints_the_joint_into_the_png() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    session.objects[0].cells.insert((0, 0, 0), 1);
    session
        .skeleton
        .add("root", None, [0.5, 0.5, 0.5], None)
        .unwrap();
    session.target = Vec3::new(0.5, 0.5, 0.5);
    session.distance = 8.0;
    session.yaw = 0.0;
    session.pitch = 0.0;
    let mut image = Image::new(16, 16);
    image.samples = 1;
    session.show_bones = false;
    let plain = encode_png(&image, &session).unwrap();
    session.show_bones = true;
    let marked = encode_png(&image, &session).unwrap();
    assert_ne!(plain, marked);
}

fn posed_block(scale: [f32; 3]) -> Session {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    for x in 0..2 {
        for y in 0..2 {
            for z in 0..2 {
                voxel(&mut session, (x, y, z), 1);
            }
        }
    }
    session
        .add_bone("lid", None, [0.0, 0.0, 0.0], None)
        .unwrap();
    session.bind_object(0, Some("lid")).unwrap();
    let mut pose = crate::rig::Pose::new();
    pose.insert(
        "lid".into(),
        crate::rig::BonePose {
            rotation: crate::rig::IDENTITY_ROTATION,
            scale,
        },
    );
    session.set_pose(pose).unwrap();
    session
}

#[test]
fn a_grown_part_is_resampled_without_holes() {
    let world = build_world(&posed_block([2.0, 2.0, 2.0]));
    assert_eq!(world.count, 64);
    assert_eq!((world.min, world.max), ([0, 0, 0], [3, 3, 3]));
}

#[test]
fn a_part_flattened_to_zero_is_left_out() {
    let world = build_world(&posed_block([1.0, 1.0, 0.0]));
    assert_eq!(world.count, 0);
    let world = build_world(&posed_block([1.0, 1.0, 0.5]));
    assert_eq!(world.count, 4);
}
