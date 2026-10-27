//! Path-traced shading: diffuse, glass, emissive, and sky.

use super::world::*;
use super::*;

pub(super) fn radiance(
    world: &World,
    session: &Shade,
    origin: Vec3,
    dir: Vec3,
    bounce: i32,
    rng: &mut Rng,
) -> [f32; 3] {
    let bounces = session.max_bounces.clamp(1, 8);
    if bounce > bounces {
        return [0.0; 3];
    }
    let Some(hit) = march(world, origin, dir) else {
        return sky(session, dir, bounce == 0);
    };
    let material = session.materials[hit.slot as usize];
    let albedo = linear_rgb(session.palette[hit.slot as usize]);
    let color = match material.ty {
        MaterialType::Emissive => {
            // Next-event estimation already samples these faces. Counting them
            // again on a bounce would light the scene twice.
            if bounce > 0 && session.stochastic_gi {
                [0.0; 3]
            } else {
                scale(albedo, emission_gain(&material))
            }
        }
        MaterialType::Metal => {
            let rough = material.roughness.clamp(0.0, 1.0);
            let metal = material.metallic.clamp(0.0, 1.0);
            if metal <= 0.0 || rng.next_f32() >= metal {
                diffuse(world, session, &hit, albedo, dir, bounce, rng)
            } else if bounce == bounces {
                [0.0; 3]
            } else {
                let view = -dir;
                let facet = ggx_sample_vndf(hit.normal, view, rough, rng);
                let wi = reflect(dir, facet);
                if wi.dot(hit.normal) <= 0.0 {
                    [0.0; 3]
                } else {
                    let next =
                        radiance(world, session, hit.point + wi * 0.001, wi, bounce + 1, rng);
                    mul(albedo, next)
                }
            }
        }
        MaterialType::Glass => glass(world, session, &hit, albedo, &material, dir, bounce, rng),
        MaterialType::Diffuse => diffuse(world, session, &hit, albedo, dir, bounce, rng),
    };
    if bounce > 0 {
        clamp_indirect(color, session.indirect_clamp)
    } else {
        color
    }
}

pub(super) fn diffuse(
    world: &World,
    session: &Shade,
    hit: &Hit,
    albedo: [f32; 3],
    _dir: Vec3,
    bounce: i32,
    rng: &mut Rng,
) -> [f32; 3] {
    let sun = sun_radiance(session);
    let ndotl = hit.normal.dot(session.sun_dir()).max(0.0);
    let mut color = if ndotl > 0.0 && !occluded(world, hit.point, session.sun_dir(), f32::MAX) {
        scale(mul(albedo, sun), ndotl)
    } else {
        [0.0; 3]
    };
    if session.stochastic_gi && bounce < session.max_bounces.clamp(1, 8) {
        if !world.lights.is_empty() {
            color = add(color, nee(world, session, hit, albedo, bounce, rng));
        }
        let wi = cosine_hemisphere(hit.normal, rng);
        let next = radiance(world, session, hit.point + wi * 0.001, wi, bounce + 1, rng);
        color = add(color, mul(albedo, next));
    } else {
        let ambient = scale(sky(session, hit.normal, false), 0.35);
        color = add(color, mul(albedo, ambient));
    }
    color
}

pub(super) fn glass(
    world: &World,
    session: &Shade,
    hit: &Hit,
    albedo: [f32; 3],
    material: &Material,
    dir: Vec3,
    bounce: i32,
    rng: &mut Rng,
) -> [f32; 3] {
    if session.glass_mode == GlassMode::Absorption {
        let tint = lerp(
            albedo,
            [1.0, 1.0, 1.0],
            material.transparency.clamp(0.0, 1.0),
        );
        let next = radiance(
            world,
            session,
            hit.point + dir * 0.002,
            dir,
            bounce + 1,
            rng,
        );
        return mul(tint, next);
    }
    let ior = material.ior.clamp(1.0, 3.0);
    let rough = material.roughness.clamp(0.0, 1.0);
    let entering = dir.dot(hit.normal) < 0.0;
    let (mut n, eta) = if entering {
        (hit.normal, 1.0 / ior)
    } else {
        (-hit.normal, ior)
    };
    if rough > 0.02 {
        n = ggx_sample_vndf(n, -dir, rough, rng);
    }
    let cos_i = (-dir).dot(n).clamp(0.0, 1.0);
    let fresnel = schlick(cos_i, ior);
    let reflect_dir = reflect(dir, n);
    if rng.next_f32() < fresnel {
        return radiance(
            world,
            session,
            hit.point + reflect_dir * 0.001,
            reflect_dir,
            bounce + 1,
            rng,
        );
    }
    let Some(refract_dir) = refract(dir, n, eta) else {
        return radiance(
            world,
            session,
            hit.point + reflect_dir * 0.001,
            reflect_dir,
            bounce + 1,
            rng,
        );
    };
    radiance(
        world,
        session,
        hit.point + refract_dir * 0.001,
        refract_dir,
        bounce + 1,
        rng,
    )
}

pub(super) fn nee(
    world: &World,
    session: &Shade,
    hit: &Hit,
    albedo: [f32; 3],
    bounce: i32,
    rng: &mut Rng,
) -> [f32; 3] {
    let candidates = session.nee_ris.clamp(1, 32) as u32;
    let mut chosen: Option<&Light> = None;
    let mut chosen_w = 0.0;
    let mut total = 0.0;
    for _ in 0..candidates {
        let light = pick_light(world, session.power_weighted_nee, rng);
        let to = light.point - hit.point;
        let dist2 = to.length_squared().max(1.0e-4);
        let wi = to.normalize_or_zero();
        let cos_l = light.normal.dot(-wi).max(0.0);
        let cos_h = hit.normal.dot(wi).max(0.0);
        let weight = light.power * cos_l * cos_h / dist2;
        total += weight;
        if rng.next_f32() * (chosen_w + weight) <= weight || chosen.is_none() {
            chosen = Some(light);
            chosen_w = weight;
        }
    }
    let Some(light) = chosen else {
        return [0.0; 3];
    };
    let to = light.point - hit.point;
    let dist = to.length().max(1.0e-3);
    let wi = to / dist;
    if occluded(world, hit.point, wi, dist - 0.02) {
        return [0.0; 3];
    }
    let cos_l = light.normal.dot(-wi).max(0.0);
    let cos_h = hit.normal.dot(wi).max(0.0);
    let mut contrib = scale(mul(albedo, light.radiance), cos_l * cos_h / (dist * dist));
    if candidates > 1 && chosen_w > 0.0 {
        contrib = scale(contrib, total / (candidates as f32 * chosen_w.max(1.0e-6)));
    }
    if bounce > 0 {
        clamp_indirect(contrib, session.indirect_clamp)
    } else {
        contrib
    }
}

pub(super) fn pick_light<'a>(world: &'a World, power_weighted: bool, rng: &mut Rng) -> &'a Light {
    if !power_weighted || world.lights.len() == 1 {
        return &world.lights
            [(rng.next_f32() * world.lights.len() as f32) as usize % world.lights.len()];
    }
    let sum: f32 = world.lights.iter().map(|light| light.power).sum();
    let mut pick = rng.next_f32() * sum.max(1.0e-6);
    for light in &world.lights {
        pick -= light.power;
        if pick <= 0.0 {
            return light;
        }
    }
    world.lights.last().expect("lights")
}

pub(super) fn camera_basis(eye: Vec3, target: Vec3) -> (Vec3, Vec3, Vec3) {
    let forward = (target - eye).normalize_or_zero();
    let world_up = if forward.dot(Vec3::Z).abs() > 0.95 {
        Vec3::Y
    } else {
        Vec3::Z
    };
    let right = forward.cross(world_up).normalize_or_zero();
    let up = right.cross(forward).normalize_or_zero();
    (forward, right, up)
}

pub(super) fn primary_dir(
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    eye: Vec3,
    target: Vec3,
    rng: &mut Rng,
) -> Vec3 {
    let (forward, right, up) = camera_basis(eye, target);
    let aspect = width as f32 / height.max(1) as f32;
    let tan = (FOV_Y * 0.5).tan();
    let jitter_x = rng.next_f32() - 0.5;
    let jitter_y = rng.next_f32() - 0.5;
    let u = ((x as f32 + 0.5 + jitter_x) / width as f32) * 2.0 - 1.0;
    let v = 1.0 - ((y as f32 + 0.5 + jitter_y) / height as f32) * 2.0;
    (forward + right * (u * aspect * tan) + up * (v * tan)).normalize_or_zero()
}

/// Parallel primary ray. The target plane matches the perspective frame at
/// `distance`, and the ray direction is the camera forward for every pixel.
pub(super) fn ortho_primary(
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    eye: Vec3,
    target: Vec3,
    rng: &mut Rng,
) -> (Vec3, Vec3) {
    let (forward, right, up) = camera_basis(eye, target);
    let aspect = width as f32 / height.max(1) as f32;
    let tan = (FOV_Y * 0.5).tan();
    let distance = eye.distance(target);
    let half_h = distance * tan;
    let half_w = half_h * aspect;
    let jitter_x = rng.next_f32() - 0.5;
    let jitter_y = rng.next_f32() - 0.5;
    let u = ((x as f32 + 0.5 + jitter_x) / width as f32) * 2.0 - 1.0;
    let v = 1.0 - ((y as f32 + 0.5 + jitter_y) / height as f32) * 2.0;
    let origin = eye + right * (u * half_w) + up * (v * half_h);
    (origin, forward)
}

/// Unjittered ray through the centre of a pixel, perspective or parallel.
pub(super) fn center_ray(
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    eye: Vec3,
    target: Vec3,
    orthographic: bool,
) -> (Vec3, Vec3) {
    let (forward, right, up) = camera_basis(eye, target);
    let aspect = width as f32 / height.max(1) as f32;
    let tan = (FOV_Y * 0.5).tan();
    let u = ((x as f32 + 0.5) / width as f32) * 2.0 - 1.0;
    let v = 1.0 - ((y as f32 + 0.5) / height as f32) * 2.0;
    if orthographic {
        let half_h = eye.distance(target) * tan;
        let origin = eye + right * (u * half_h * aspect) + up * (v * half_h);
        (origin, forward)
    } else {
        let dir = forward + right * (u * aspect * tan) + up * (v * tan);
        (eye, dir.normalize_or_zero())
    }
}

pub(super) fn lens_ray(
    session: &Shade,
    eye: Vec3,
    target: Vec3,
    dir: Vec3,
    rng: &mut Rng,
) -> (Vec3, Vec3) {
    if session.aperture <= 0.0 {
        return (eye, dir);
    }
    let forward = (target - eye).normalize_or_zero();
    let world_up = if forward.dot(Vec3::Z).abs() > 0.95 {
        Vec3::Y
    } else {
        Vec3::Z
    };
    let right = forward.cross(world_up).normalize_or_zero();
    let up = right.cross(forward);
    let radius = session.aperture.sqrt() * 0.05 * rng.next_f32().sqrt();
    let angle = rng.next_f32() * std::f32::consts::TAU;
    let lens = eye + right * (angle.cos() * radius) + up * (angle.sin() * radius);
    let focus = eye + dir * session.focus_distance.max(0.05);
    (lens, (focus - lens).normalize_or_zero())
}

pub(super) fn sun_radiance(session: &Shade) -> [f32; 3] {
    scale(session.sun_color, session.sun_intensity.max(0.0))
}

const EARTH_RADIUS: f32 = 6_371_000.0;
const ATMOSPHERE_TOP: f32 = 6_471_000.0;
const RAYLEIGH_SIGMA: [f32; 3] = [5.802e-6, 13.558e-6, 33.1e-6];
const MIE_SCATTER: f32 = 3.996e-6;
const MIE_ABSORB: f32 = 4.4e-6;
const RAYLEIGH_SCALE: f32 = 8_000.0;
const MIE_SCALE: f32 = 1_200.0;
const MIE_G: f32 = 0.76;
const SKY_VIEW_SAMPLES: i32 = 32;
const SKY_SUN_SAMPLES: i32 = 8;

fn ray_outer_sphere(point: Vec3, dir: Vec3, radius: f32) -> f32 {
    let b = point.dot(dir);
    let c = point.length_squared() - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 {
        -1.0
    } else {
        -b + disc.sqrt()
    }
}

fn atmosphere_altitude(point: Vec3) -> f32 {
    let r2 = point.length_squared();
    (r2 - EARTH_RADIUS * EARTH_RADIUS) / (r2.sqrt() + EARTH_RADIUS)
}

fn atmosphere_transmittance(point: Vec3, omega: Vec3) -> [f32; 3] {
    let t_top = ray_outer_sphere(point, omega, ATMOSPHERE_TOP);
    if t_top <= 0.0 {
        return [0.0; 3];
    }
    let t_ground = ray_outer_sphere(point, omega, EARTH_RADIUS);
    if t_ground > 1.0 && t_ground < t_top {
        return [0.0; 3];
    }
    let dt = t_top / SKY_SUN_SAMPLES as f32;
    let mut od_r = [0.0; 3];
    let mut od_m = 0.0;
    for i in 0..SKY_SUN_SAMPLES {
        let t = (i as f32 + 0.5) * dt;
        let height = atmosphere_altitude(point + omega * t).max(0.0);
        od_r = add(
            od_r,
            scale(RAYLEIGH_SIGMA, (-height / RAYLEIGH_SCALE).exp() * dt),
        );
        od_m += (MIE_SCATTER + MIE_ABSORB) * (-height / MIE_SCALE).exp() * dt;
    }
    [
        (-(od_r[0] + od_m)).exp(),
        (-(od_r[1] + od_m)).exp(),
        (-(od_r[2] + od_m)).exp(),
    ]
}

/// Sun irradiance that lights the atmospheric sky, times `sun_color` and
/// `sky_intensity`. It does not follow the Sun slider or the disc size, so the
/// sky stays lit with the sun off. At the default 30° sun the horizon is about
/// as bright as the flat sky's.
const SKY_SUN_IRRADIANCE: f32 = 10.0;
const GROUND: [f32; 3] = [0.28, 0.26, 0.24];
const HORIZON_WHITE: [f32; 3] = [0.75, 0.78, 0.82];

/// Nishita single-scatter sky. `sun_irradiance` is top-of-atmosphere.
/// Below the horizon every path ends in the planet, so it is black; see
/// `atmospheric_sky` for the ground.
pub(super) fn nishita_sky(view: Vec3, sun: Vec3, sun_irradiance: [f32; 3]) -> [f32; 3] {
    let view = view.normalize_or_zero();
    let sun = sun.normalize_or_zero();
    let origin = Vec3::new(0.0, 0.0, EARTH_RADIUS);
    let t_top = ray_outer_sphere(origin, view, ATMOSPHERE_TOP);
    if t_top <= 0.0 {
        return [0.0; 3];
    }
    let sun_irr = sun_irradiance;
    let cos_theta = view.dot(sun);
    let cos2 = cos_theta * cos_theta;
    const INV_PI: f32 = 0.318_309_886_18;
    let phase_r = (3.0 / 16.0) * INV_PI * (1.0 + cos2);
    let g2 = MIE_G * MIE_G;
    let phase_m = (3.0 * 0.125 * INV_PI) * (1.0 - g2) * (1.0 + cos2)
        / ((2.0 + g2) * (1.0 + g2 - 2.0 * MIE_G * cos_theta).powf(1.5));
    let dt = t_top / SKY_VIEW_SAMPLES as f32;
    let mut luminance = [0.0; 3];
    let mut view_t = [1.0, 1.0, 1.0];
    for i in 0..SKY_VIEW_SAMPLES {
        let t = (i as f32 + 0.5) * dt;
        let sample = origin + view * t;
        let height = atmosphere_altitude(sample).max(0.0);
        let density_r = (-height / RAYLEIGH_SCALE).exp();
        let density_m = (-height / MIE_SCALE).exp();
        let to_sun = atmosphere_transmittance(sample, sun);
        let scatter = add(
            scale(RAYLEIGH_SIGMA, density_r * phase_r),
            [MIE_SCATTER * density_m * phase_m; 3],
        );
        let incoming = mul(mul(view_t, scatter), mul(to_sun, sun_irr));
        luminance = add(luminance, scale(incoming, dt));
        let ext = add(
            scale(RAYLEIGH_SIGMA, density_r),
            [(MIE_SCATTER + MIE_ABSORB) * density_m; 3],
        );
        view_t = [
            view_t[0] * (-ext[0] * dt).exp(),
            view_t[1] * (-ext[1] * dt).exp(),
            view_t[2] * (-ext[2] * dt).exp(),
        ];
    }
    luminance
}

/// The Nishita sky above the horizon. Below it, the horizon colour fades into
/// the flat sky's ground colour, lit as brightly as that horizon.
pub(super) fn atmospheric_sky(session: &Shade, dir: Vec3) -> [f32; 3] {
    let sun = session.sun_dir();
    let irradiance = scale(
        session.sun_color,
        SKY_SUN_IRRADIANCE * session.sky_intensity.max(0.0),
    );
    if dir.z >= 0.0 {
        return nishita_sky(dir, sun, irradiance);
    }
    let flat = Vec3::new(dir.x, dir.y, 0.0);
    let flat = if flat.length_squared() > 1.0e-8 {
        flat.normalize()
    } else {
        Vec3::X
    };
    let horizon = nishita_sky(flat, sun, irradiance);
    let level = (horizon[0] + horizon[1] + horizon[2])
        / (HORIZON_WHITE[0] + HORIZON_WHITE[1] + HORIZON_WHITE[2]);
    lerp(scale(GROUND, level), horizon, (dir.z * 8.0).exp())
}

/// Visible GGX normal (Heitz 2018). `view` points back along the ray (`-dir`).
/// Roughness 0 returns `normal`.
pub(super) fn ggx_sample_vndf(normal: Vec3, view: Vec3, roughness: f32, rng: &mut Rng) -> Vec3 {
    let alpha = roughness * roughness;
    let helper = if normal.x.abs() > 0.5 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let tangent = normal.cross(helper).normalize_or_zero();
    let bitangent = normal.cross(tangent);
    let view_t = Vec3::new(view.dot(tangent), view.dot(bitangent), view.dot(normal));
    let stretched = Vec3::new(alpha * view_t.x, alpha * view_t.y, view_t.z).normalize_or_zero();
    let lensq = stretched.x * stretched.x + stretched.y * stretched.y;
    let t1 = if lensq > 0.0 {
        Vec3::new(-stretched.y, stretched.x, 0.0) / lensq.sqrt()
    } else {
        Vec3::X
    };
    let t2 = stretched.cross(t1);
    let radius = rng.next_f32().sqrt();
    let phi = std::f32::consts::TAU * rng.next_f32();
    let p1 = radius * phi.cos();
    let mut p2 = radius * phi.sin();
    let cap = 0.5 * (1.0 + stretched.z);
    p2 = (1.0 - cap) * (1.0 - p1 * p1).max(0.0).sqrt() + cap * p2;
    let hemisphere = t1 * p1 + t2 * p2 + stretched * (1.0 - p1 * p1 - p2 * p2).max(0.0).sqrt();
    let unstretched = Vec3::new(
        alpha * hemisphere.x,
        alpha * hemisphere.y,
        hemisphere.z.max(0.0),
    )
    .normalize_or_zero();
    (tangent * unstretched.x + bitangent * unstretched.y + normal * unstretched.z)
        .normalize_or_zero()
}

pub(super) fn sky(session: &Shade, dir: Vec3, include_sun: bool) -> [f32; 3] {
    let dir = dir.normalize_or_zero();
    let mut color = if session.atmospheric_sky {
        atmospheric_sky(session, dir)
    } else {
        let up = dir.z.clamp(-1.0, 1.0);
        let horizon = (-up.abs() * session.horizon_softness.max(0.0)).exp();
        let zenith = [0.55, 0.62, 0.72];
        let base = if up >= 0.0 {
            lerp(zenith, HORIZON_WHITE, horizon)
        } else {
            lerp(GROUND, HORIZON_WHITE, horizon)
        };
        scale(base, session.sky_intensity.max(0.0))
    };
    if include_sun {
        let cos = dir
            .normalize_or_zero()
            .dot(session.sun_dir())
            .clamp(-1.0, 1.0);
        let radius = (session.sun_disc.max(0.1).to_radians() * 0.5).cos();
        if cos >= radius {
            color = add(color, sun_radiance(session));
        }
    }
    color
}

pub(super) fn first_guides(
    world: &World,
    session: &Shade,
    origin: Vec3,
    dir: Vec3,
) -> ([f32; 3], [f32; 3]) {
    match march(world, origin, dir) {
        Some(hit) => (
            linear_rgb(session.palette[hit.slot as usize]),
            [hit.normal.x, hit.normal.y, hit.normal.z],
        ),
        None => ([0.0; 3], [0.0; 3]),
    }
}

pub(super) fn denoise(image: &Image) -> Vec<[f32; 3]> {
    let w = image.width as i32;
    let h = image.height as i32;
    let mut out = vec![[0.0; 3]; image.shown.len()];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            let mut acc = [0.0; 3];
            let mut weight = 0.0;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let nx = x + dx;
                    let ny = y + dy;
                    if nx < 0 || ny < 0 || nx >= w || ny >= h {
                        continue;
                    }
                    let j = (ny * w + nx) as usize;
                    let diff = sub(image.albedo[i], image.albedo[j]);
                    let dist = (diff[0] * diff[0] + diff[1] * diff[1] + diff[2] * diff[2]).sqrt();
                    let wgt = (-dist * 12.0).exp();
                    acc = add(acc, scale(image.shown[j], wgt));
                    weight += wgt;
                }
            }
            out[i] = scale(acc, 1.0 / weight.max(1.0e-6));
        }
    }
    out
}

pub(super) fn cosine_hemisphere(normal: Vec3, rng: &mut Rng) -> Vec3 {
    let u1 = rng.next_f32().clamp(1.0e-6, 1.0);
    let u2 = rng.next_f32();
    let r = u1.sqrt();
    let phi = std::f32::consts::TAU * u2;
    let local = Vec3::new(r * phi.cos(), r * phi.sin(), (1.0 - u1).sqrt());
    let tangent = if normal.z.abs() < 0.9 {
        normal.cross(Vec3::Z).normalize()
    } else {
        normal.cross(Vec3::X).normalize()
    };
    let bitangent = normal.cross(tangent);
    (tangent * local.x + bitangent * local.y + normal * local.z).normalize_or_zero()
}

pub(super) fn reflect(incoming: Vec3, normal: Vec3) -> Vec3 {
    (incoming - normal * (2.0 * incoming.dot(normal))).normalize_or_zero()
}

pub(super) fn refract(dir: Vec3, normal: Vec3, eta: f32) -> Option<Vec3> {
    let cos_i = (-dir).dot(normal).clamp(-1.0, 1.0);
    let sin2_t = eta * eta * (1.0 - cos_i * cos_i);
    if sin2_t > 1.0 {
        return None;
    }
    let cos_t = (1.0 - sin2_t).sqrt();
    Some((dir * eta + normal * (eta * cos_i - cos_t)).normalize_or_zero())
}

pub(super) fn schlick(cos: f32, ior: f32) -> f32 {
    let r0 = ((1.0 - ior) / (1.0 + ior)).powi(2);
    r0 + (1.0 - r0) * (1.0 - cos).powi(5)
}

pub(super) fn clamp_indirect(color: [f32; 3], limit: f32) -> [f32; 3] {
    if limit <= 0.0 {
        return color;
    }
    let peak = color[0].max(color[1]).max(color[2]);
    if peak <= limit {
        color
    } else {
        scale(color, limit / peak)
    }
}

pub(super) fn aces(x: f32) -> f32 {
    let x = x.max(0.0);
    ((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14)).clamp(0.0, 1.0)
}

pub(super) fn to_srgb(linear: f32) -> u8 {
    let linear = linear.clamp(0.0, 1.0);
    let encoded = if linear <= 0.0031308 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}
