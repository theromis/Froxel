
struct Uniforms {
    eye: vec4<f32>,
    aim: vec4<f32>,
    sun_dir: vec4<f32>,
    sun_color: vec4<f32>,
    origin: vec4<i32>,
    dims: vec4<u32>,
    misc0: vec4<u32>,
    misc1: vec4<u32>,
    cam: vec4<f32>,
    tone: vec4<f32>,
}
struct Material {
    ty: u32,
    emission_power: u32,
    roughness: f32,
    metallic: f32,
    emission: f32,
    transparency: f32,
    ior: f32,
    pad: f32,
}
struct Light {
    point: vec4<f32>,
    normal: vec4<f32>,
    radiance: vec4<f32>,
}
struct Pixel {
    sum: vec4<f32>,
    shown: vec4<f32>,
    albedo: vec4<f32>,
    normal: vec4<f32>,
}
struct Hit {
    found: bool,
    slot: u32,
    point: vec3<f32>,
    normal: vec3<f32>,
}
struct Sample {
    color: vec3<f32>,
    albedo: vec3<f32>,
    normal: vec3<f32>,
}

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var<storage, read> materials: array<Material>;
@group(0) @binding(2) var<storage, read> palette: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> voxels: array<u32>;
@group(0) @binding(4) var<storage, read> lights: array<Light>;
@group(0) @binding(5) var<storage, read_write> pixels: array<Pixel>;
@group(0) @binding(6) var display_tex: texture_storage_2d<rgba16float, write>;

const MAX_STEPS: i32 = 384;

fn hash(x: u32, y: u32, sample: u32) -> u32 {
    var n = x * 374761393u + y * 668265263u + sample * 1440678137u + bitcast<u32>(u.tone.w);
    n = (n ^ (n >> 13u)) * 1274126177u;
    return n ^ (n >> 16u);
}

fn rnd(state: ptr<function, u32>) -> f32 {
    *state = (*state) * 747796405u + 2891336453u;
    let word = (((*state) >> (((*state) >> 28u) + 4u)) ^ (*state)) * 277803737u;
    let bits = (word >> 22u) ^ word;
    return f32(bits >> 8u) / 16777216.0;
}

fn safe_norm(v: vec3<f32>) -> vec3<f32> {
    let len2 = dot(v, v);
    if (len2 < 1e-8) { return vec3<f32>(0.0); }
    return v / sqrt(len2);
}

fn slot_at(cell: vec3<i32>) -> u32 {
    let p = cell - u.origin.xyz;
    if (any(p < vec3<i32>(0)) || any(vec3<u32>(p) >= u.dims.xyz)) { return 0u; }
    let i = u32(p.x) + u.dims.x * (u32(p.y) + u.dims.y * u32(p.z));
    if (i >= arrayLength(&voxels)) { return 0u; }
    return voxels[i];
}

fn box_t(origin: vec3<f32>, dir: vec3<f32>) -> f32 {
    var t0 = 0.0;
    var t1 = 1e30;
    let lo = vec3<f32>(u.origin.xyz);
    let hi = lo + vec3<f32>(u.dims.xyz);
    for (var axis = 0; axis < 3; axis++) {
        let d = dir[axis];
        let o = origin[axis];
        if (abs(d) < 1e-8) {
            if (o < lo[axis] || o > hi[axis]) { return -1.0; }
            continue;
        }
        var a = (lo[axis] - o) / d;
        var b = (hi[axis] - o) / d;
        if (a > b) {
            let s = a;
            a = b;
            b = s;
        }
        t0 = max(t0, a);
        t1 = min(t1, b);
        if (t0 > t1) { return -1.0; }
    }
    if (t1 < 0.0) { return -1.0; }
    return max(t0, 0.0);
}

fn march(origin_in: vec3<f32>, dir: vec3<f32>) -> Hit {
    var miss: Hit;
    miss.found = false;
    miss.slot = 0u;
    miss.point = origin_in;
    miss.normal = vec3<f32>(0.0, 0.0, 1.0);
    if (dot(dir, dir) < 1e-8) { return miss; }
    let entry = box_t(origin_in, dir);
    if (entry < 0.0) { return miss; }
    let origin = origin_in + dir * entry;
    var cell = vec3<i32>(floor(origin));
    let step = vec3<i32>(select(vec3<i32>(-1), vec3<i32>(1), dir >= vec3<f32>(0.0)));
    let delta = vec3<f32>(
        select(1.0 / abs(dir.x), 1e30, abs(dir.x) < 1e-8),
        select(1.0 / abs(dir.y), 1e30, abs(dir.y) < 1e-8),
        select(1.0 / abs(dir.z), 1e30, abs(dir.z) < 1e-8),
    );
    var tmax: vec3<f32>;
    for (var axis = 0; axis < 3; axis++) {
        var start = f32(cell[axis]);
        if (step[axis] > 0) { start = f32(cell[axis] + 1); }
        if (abs(dir[axis]) < 1e-8) {
            tmax[axis] = 1e30;
        } else {
            tmax[axis] = (start - origin[axis]) / dir[axis];
        }
    }
    var normal = -safe_norm(dir);
    let first = slot_at(cell);
    if (first != 0u) {
        var hit: Hit;
        hit.found = true;
        hit.slot = first;
        hit.normal = normal;
        hit.point = vec3<f32>(cell) + vec3<f32>(0.5) + normal * 0.5;
        return hit;
    }
    for (var n = 0; n < MAX_STEPS; n++) {
        var axis = 0;
        if (tmax.y <= tmax.x && tmax.y <= tmax.z) { axis = 1; }
        if (tmax.z <= tmax.x && tmax.z <= tmax.y) { axis = 2; }
        cell[axis] = cell[axis] + step[axis];
        tmax[axis] = tmax[axis] + delta[axis];
        normal = vec3<f32>(0.0);
        normal[axis] = -f32(step[axis]);
        if (cell[axis] < u.origin[axis] - 1 || cell[axis] > u.origin[axis] + i32(u.dims[axis])) {
            return miss;
        }
        let slot = slot_at(cell);
        if (slot != 0u) {
            var hit: Hit;
            hit.found = true;
            hit.slot = slot;
            hit.normal = normal;
            hit.point = vec3<f32>(cell) + vec3<f32>(0.5) + normal * 0.5;
            return hit;
        }
    }
    return miss;
}

fn occluded(origin: vec3<f32>, dir: vec3<f32>, max_dist: f32) -> bool {
    let hit = march(origin + dir * 0.002, dir);
    if (!hit.found) { return false; }
    return dot(hit.point - origin, dir) < max_dist;
}

const EARTH_RADIUS: f32 = 6371000.0;
const ATMOSPHERE_TOP: f32 = 6471000.0;
const RAYLEIGH_SIGMA: vec3<f32> = vec3<f32>(5.802e-6, 13.558e-6, 33.1e-6);
const MIE_SCATTER: f32 = 3.996e-6;
const MIE_ABSORB: f32 = 4.4e-6;
const RAYLEIGH_SCALE: f32 = 8000.0;
const MIE_SCALE: f32 = 1200.0;
const MIE_G: f32 = 0.76;
const SKY_VIEW_SAMPLES: i32 = 32;
const SKY_SUN_SAMPLES: i32 = 8;

fn ray_outer_sphere(point: vec3<f32>, dir: vec3<f32>, radius: f32) -> f32 {
    let b = dot(point, dir);
    let c = dot(point, point) - radius * radius;
    let disc = b * b - c;
    if (disc < 0.0) { return -1.0; }
    return -b + sqrt(disc);
}

fn atmosphere_altitude(point: vec3<f32>) -> f32 {
    let r2 = dot(point, point);
    return (r2 - EARTH_RADIUS * EARTH_RADIUS) / (sqrt(r2) + EARTH_RADIUS);
}

fn atmosphere_transmittance(point: vec3<f32>, omega: vec3<f32>) -> vec3<f32> {
    let t_top = ray_outer_sphere(point, omega, ATMOSPHERE_TOP);
    if (t_top <= 0.0) { return vec3<f32>(0.0); }
    let t_ground = ray_outer_sphere(point, omega, EARTH_RADIUS);
    if (t_ground > 1.0 && t_ground < t_top) { return vec3<f32>(0.0); }
    let dt = t_top / f32(SKY_SUN_SAMPLES);
    var od_r = vec3<f32>(0.0);
    var od_m = 0.0;
    for (var i = 0; i < SKY_SUN_SAMPLES; i++) {
        let t = (f32(i) + 0.5) * dt;
        let height = max(atmosphere_altitude(point + omega * t), 0.0);
        od_r = od_r + RAYLEIGH_SIGMA * exp(-height / RAYLEIGH_SCALE) * dt;
        od_m = od_m + (MIE_SCATTER + MIE_ABSORB) * exp(-height / MIE_SCALE) * dt;
    }
    return exp(-od_r - vec3<f32>(od_m));
}

const SKY_SUN_IRRADIANCE: f32 = 10.0;
const GROUND: vec3<f32> = vec3<f32>(0.28, 0.26, 0.24);
const HORIZON_WHITE: vec3<f32> = vec3<f32>(0.75, 0.78, 0.82);

fn nishita_sky(view_in: vec3<f32>, sun_in: vec3<f32>, sun_irr: vec3<f32>) -> vec3<f32> {
    let view = safe_norm(view_in);
    let sun = safe_norm(sun_in);
    let origin = vec3<f32>(0.0, 0.0, EARTH_RADIUS);
    let t_top = ray_outer_sphere(origin, view, ATMOSPHERE_TOP);
    if (t_top <= 0.0) { return vec3<f32>(0.0); }
    let cos_theta = dot(view, sun);
    let cos2 = cos_theta * cos_theta;
    let phase_r = (3.0 / 16.0) * 0.31830988618 * (1.0 + cos2);
    let g2 = MIE_G * MIE_G;
    let phase_m = (3.0 * 0.125 * 0.31830988618) * (1.0 - g2) * (1.0 + cos2)
        / ((2.0 + g2) * pow(1.0 + g2 - 2.0 * MIE_G * cos_theta, 1.5));
    let dt = t_top / f32(SKY_VIEW_SAMPLES);
    var luminance = vec3<f32>(0.0);
    var view_t = vec3<f32>(1.0);
    for (var i = 0; i < SKY_VIEW_SAMPLES; i++) {
        let t = (f32(i) + 0.5) * dt;
        let sample = origin + view * t;
        let height = max(atmosphere_altitude(sample), 0.0);
        let density_r = exp(-height / RAYLEIGH_SCALE);
        let density_m = exp(-height / MIE_SCALE);
        let to_sun = atmosphere_transmittance(sample, sun);
        let scatter = RAYLEIGH_SIGMA * density_r * phase_r + vec3<f32>(MIE_SCATTER * density_m * phase_m);
        luminance = luminance + view_t * scatter * to_sun * sun_irr * dt;
        let ext = RAYLEIGH_SIGMA * density_r + vec3<f32>((MIE_SCATTER + MIE_ABSORB) * density_m);
        view_t = view_t * exp(-ext * dt);
    }
    return luminance;
}

// Same as shade.rs `atmospheric_sky`.
fn atmospheric_sky(dir: vec3<f32>) -> vec3<f32> {
    let irradiance = u.sun_color.xyz * (SKY_SUN_IRRADIANCE * max(u.cam.z, 0.0));
    if (dir.z >= 0.0) { return nishita_sky(dir, u.sun_dir.xyz, irradiance); }
    var flat = vec3<f32>(dir.x, dir.y, 0.0);
    if (dot(flat, flat) > 1.0e-8) { flat = normalize(flat); } else { flat = vec3<f32>(1.0, 0.0, 0.0); }
    let horizon = nishita_sky(flat, u.sun_dir.xyz, irradiance);
    let level = (horizon.x + horizon.y + horizon.z)
        / (HORIZON_WHITE.x + HORIZON_WHITE.y + HORIZON_WHITE.z);
    return mix(GROUND * level, horizon, exp(dir.z * 8.0));
}

fn ggx_sample_vndf(n: vec3<f32>, view: vec3<f32>, roughness: f32, rng: ptr<function, u32>) -> vec3<f32> {
    let a = roughness * roughness;
    var helper = vec3<f32>(1.0, 0.0, 0.0);
    if (abs(n.x) > 0.5) { helper = vec3<f32>(0.0, 1.0, 0.0); }
    let t1 = safe_norm(cross(n, helper));
    let t2 = cross(n, t1);
    let ve = vec3<f32>(dot(view, t1), dot(view, t2), dot(view, n));
    let vh = safe_norm(vec3<f32>(a * ve.x, a * ve.y, ve.z));
    let lensq = vh.x * vh.x + vh.y * vh.y;
    var basis = vec3<f32>(1.0, 0.0, 0.0);
    if (lensq > 0.0) { basis = vec3<f32>(-vh.y, vh.x, 0.0) / sqrt(lensq); }
    let side = cross(vh, basis);
    let radius = sqrt(rnd(rng));
    let phi = 6.2831853 * rnd(rng);
    let p1 = radius * cos(phi);
    var p2 = radius * sin(phi);
    let cap = 0.5 * (1.0 + vh.z);
    p2 = (1.0 - cap) * sqrt(max(0.0, 1.0 - p1 * p1)) + cap * p2;
    let nh = basis * p1 + side * p2 + vh * sqrt(max(0.0, 1.0 - p1 * p1 - p2 * p2));
    let ne = safe_norm(vec3<f32>(a * nh.x, a * nh.y, max(0.0, nh.z)));
    return safe_norm(t1 * ne.x + t2 * ne.y + n * ne.z);
}

fn sky(dir_in: vec3<f32>, include_sun: bool) -> vec3<f32> {
    let dir = safe_norm(dir_in);
    var color: vec3<f32>;
    if ((u.misc1.x & 8u) != 0u) {
        color = atmospheric_sky(dir);
    } else {
        let up = clamp(dir.z, -1.0, 1.0);
        let horizon = exp(-abs(up) * u.cam.w);
        let zenith = vec3<f32>(0.55, 0.62, 0.72);
        var base = mix(GROUND, HORIZON_WHITE, horizon);
        if (up >= 0.0) { base = mix(zenith, HORIZON_WHITE, horizon); }
        color = base * u.cam.z;
    }
    if (include_sun && dot(dir, u.sun_dir.xyz) >= u.tone.x) {
        color = color + u.sun_color.xyz * u.sun_color.w;
    }
    return color;
}

fn cosine(normal: vec3<f32>, rng: ptr<function, u32>) -> vec3<f32> {
    let u1 = max(rnd(rng), 1e-6);
    let u2 = rnd(rng);
    let r = sqrt(u1);
    let phi = 6.2831853 * u2;
    let local = vec3<f32>(r * cos(phi), r * sin(phi), sqrt(1.0 - u1));
    var tangent = safe_norm(cross(normal, vec3<f32>(0.0, 0.0, 1.0)));
    if (abs(normal.z) >= 0.9) {
        tangent = safe_norm(cross(normal, vec3<f32>(1.0, 0.0, 0.0)));
    }
    let bitangent = cross(normal, tangent);
    return safe_norm(tangent * local.x + bitangent * local.y + normal * local.z);
}

fn reflect_dir(incoming: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    return safe_norm(incoming - normal * (2.0 * dot(incoming, normal)));
}

fn clamp_indirect(color: vec3<f32>) -> vec3<f32> {
    let limit = u.tone.y;
    if (limit <= 0.0) { return color; }
    let peak = max(color.x, max(color.y, color.z));
    if (peak <= limit) { return color; }
    return color * (limit / peak);
}

fn primary_dir(px: vec2<u32>, rng: ptr<function, u32>) -> vec3<f32> {
    let forward = safe_norm(u.aim.xyz - u.eye.xyz);
    var world_up = vec3<f32>(0.0, 0.0, 1.0);
    if (abs(dot(forward, world_up)) > 0.95) { world_up = vec3<f32>(0.0, 1.0, 0.0); }
    let right = safe_norm(cross(forward, world_up));
    let up = safe_norm(cross(right, forward));
    let aspect = f32(u.misc0.x) / max(f32(u.misc0.y), 1.0);
    let tan_y = tan(radians(50.0) * 0.5);
    let jx = rnd(rng) - 0.5;
    let jy = rnd(rng) - 0.5;
    let uu = ((f32(px.x) + 0.5 + jx) / f32(u.misc0.x)) * 2.0 - 1.0;
    let vv = 1.0 - ((f32(px.y) + 0.5 + jy) / f32(u.misc0.y)) * 2.0;
    return safe_norm(forward + right * (uu * aspect * tan_y) + up * (vv * tan_y));
}

fn lens_origin(dir: vec3<f32>, rng: ptr<function, u32>) -> vec3<f32> {
    let eye = u.eye.xyz;
    if (u.cam.x <= 0.0) { return eye; }
    let forward = safe_norm(u.aim.xyz - eye);
    var world_up = vec3<f32>(0.0, 0.0, 1.0);
    if (abs(dot(forward, world_up)) > 0.95) { world_up = vec3<f32>(0.0, 1.0, 0.0); }
    let right = safe_norm(cross(forward, world_up));
    let up = safe_norm(cross(right, forward));
    let radius = sqrt(u.cam.x) * 0.05 * sqrt(rnd(rng));
    let angle = rnd(rng) * 6.2831853;
    return eye + right * (cos(angle) * radius) + up * (sin(angle) * radius);
}

fn nee(hit: Hit, albedo: vec3<f32>, rng: ptr<function, u32>) -> vec3<f32> {
    let count = u.misc1.z;
    if (count == 0u) { return vec3<f32>(0.0); }
    let candidates = max(u.misc1.y, 1u);
    var chosen = 0u;
    var chosen_w = 0.0;
    var total = 0.0;
    var have = false;
    for (var n = 0u; n < candidates; n++) {
        var index = u32(rnd(rng) * f32(count)) % count;
        if ((u.misc1.x & 16u) != 0u && count > 1u) {
            var sum = 0.0;
            for (var i = 0u; i < count; i++) { sum = sum + lights[i].radiance.w; }
            var pick = rnd(rng) * max(sum, 1e-6);
            index = count - 1u;
            for (var i = 0u; i < count; i++) {
                pick = pick - lights[i].radiance.w;
                if (pick <= 0.0) { index = i; break; }
            }
        }
        let light = lights[index];
        let to = light.point.xyz - hit.point;
        let dist2 = max(dot(to, to), 1e-4);
        let wi = to / sqrt(dist2);
        let cos_l = max(dot(light.normal.xyz, -wi), 0.0);
        let cos_h = max(dot(hit.normal, wi), 0.0);
        let weight = light.radiance.w * cos_l * cos_h / dist2;
        total = total + weight;
        if (!have || rnd(rng) * (chosen_w + weight) <= weight) {
            chosen = index;
            chosen_w = weight;
            have = true;
        }
    }
    let light = lights[chosen];
    let to = light.point.xyz - hit.point;
    let dist = max(length(to), 1e-3);
    let wi = to / dist;
    if (occluded(hit.point, wi, dist - 0.02)) { return vec3<f32>(0.0); }
    let cos_l = max(dot(light.normal.xyz, -wi), 0.0);
    let cos_h = max(dot(hit.normal, wi), 0.0);
    var contrib = albedo * light.radiance.xyz * (cos_l * cos_h / (dist * dist));
    if (candidates > 1u && chosen_w > 0.0) {
        contrib = contrib * (total / (f32(candidates) * max(chosen_w, 1e-6)));
    }
    return contrib;
}

fn trace_sample(px: vec2<u32>, sample_i: u32) -> Sample {
    var rng = hash(px.x, px.y, sample_i);
    var dir = primary_dir(px, &rng);
    var origin = u.eye.xyz;
    if (u.cam.x > 0.0) {
        let lens = lens_origin(dir, &rng);
        let focus = origin + dir * max(u.cam.y, 0.05);
        origin = lens;
        dir = safe_norm(focus - lens);
    }
    var throughput = vec3<f32>(1.0);
    var color = vec3<f32>(0.0);
    var albedo = vec3<f32>(0.0);
    var normal = vec3<f32>(0.0);
    let bounces = u.misc0.w;
    let gi = (u.misc1.x & 1u) != 0u;
    for (var bounce = 0u; bounce <= bounces; bounce++) {
        let hit = march(origin, dir);
        if (!hit.found) {
            var sky_c = sky(dir, bounce == 0u);
            if (bounce > 0u) { sky_c = clamp_indirect(sky_c); }
            color = color + throughput * sky_c;
            break;
        }
        let slot = min(hit.slot, 255u);
        if (bounce == 0u) {
            albedo = palette[slot].xyz;
            normal = hit.normal;
        }
        let mat = materials[slot];
        let alb = palette[slot].xyz;
        if (mat.ty == 2u) {
            var emitted = alb * mat.emission * f32(mat.emission_power) * 8.0;
            if (bounce > 0u && gi) { emitted = vec3<f32>(0.0); }
            if (bounce > 0u) { emitted = clamp_indirect(emitted); }
            color = color + throughput * emitted;
            break;
        }
        if (mat.ty == 1u && mat.metallic > 0.0 && rnd(&rng) < clamp(mat.metallic, 0.0, 1.0)) {
            if (bounce == bounces) { break; }
            let facet = ggx_sample_vndf(hit.normal, -dir, clamp(mat.roughness, 0.0, 1.0), &rng);
            let wi = reflect_dir(dir, facet);
            if (dot(wi, hit.normal) <= 0.0) { break; }
            throughput = throughput * alb;
            origin = hit.point + wi * 0.001;
            dir = wi;
            continue;
        }
        if (mat.ty == 3u) {
            if (bounce == bounces) { break; }
            if ((u.misc1.x & 32u) != 0u) {
                let tint = mix(alb, vec3<f32>(1.0), clamp(mat.transparency, 0.0, 1.0));
                throughput = throughput * tint;
                origin = hit.point + dir * 0.002;
                continue;
            }
            let ior = clamp(mat.ior, 1.0, 3.0);
            let entering = dot(dir, hit.normal) < 0.0;
            var n = hit.normal;
            var eta = ior;
            if (entering) {
                eta = 1.0 / ior;
            } else {
                n = -hit.normal;
            }
            if (mat.roughness > 0.02) {
                n = ggx_sample_vndf(n, -dir, clamp(mat.roughness, 0.0, 1.0), &rng);
            }
            let cos_i = clamp(dot(-dir, n), 0.0, 1.0);
            let r0 = pow((1.0 - ior) / (1.0 + ior), 2.0);
            let fresnel = r0 + (1.0 - r0) * pow(1.0 - cos_i, 5.0);
            let reflected = reflect_dir(dir, n);
            if (rnd(&rng) < fresnel) {
                origin = hit.point + reflected * 0.001;
                dir = reflected;
                continue;
            }
            let sin2_t = eta * eta * (1.0 - cos_i * cos_i);
            if (sin2_t > 1.0) {
                origin = hit.point + reflected * 0.001;
                dir = reflected;
                continue;
            }
            let refracted = safe_norm(dir * eta + n * (eta * cos_i - sqrt(1.0 - sin2_t)));
            origin = hit.point + refracted * 0.001;
            dir = refracted;
            continue;
        }
        let sun = u.sun_dir.xyz;
        let ndotl = max(dot(hit.normal, sun), 0.0);
        var direct = vec3<f32>(0.0);
        if (ndotl > 0.0 && !occluded(hit.point, sun, 1e20)) {
            direct = alb * u.sun_color.xyz * u.sun_color.w * ndotl;
        }
        if (gi && bounce < bounces) {
            direct = direct + nee(hit, alb, &rng);
            if (bounce > 0u) { direct = clamp_indirect(direct); }
            color = color + throughput * direct;
            let wi = cosine(hit.normal, &rng);
            throughput = throughput * alb;
            origin = hit.point + wi * 0.001;
            dir = wi;
        } else {
            direct = direct + alb * sky(hit.normal, false) * 0.35;
            if (bounce > 0u) { direct = clamp_indirect(direct); }
            color = color + throughput * direct;
            break;
        }
    }
    var out: Sample;
    out.color = color;
    out.albedo = albedo;
    out.normal = normal;
    return out;
}

fn display_of(sum: vec3<f32>, shown: vec3<f32>, color: vec3<f32>, sample_i: u32) -> vec3<f32> {
    let n = f32(sample_i + 1u);
    let weight = max(1.0 / n, u.tone.z);
    var blended = color;
    if (sample_i != 0u) { blended = mix(shown, color, weight); }
    if (u.tone.z <= 0.0) { return sum / n; }
    return blended;
}

@compute @workgroup_size(8, 8)
fn trace_main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= u.misc0.x || id.y >= u.misc0.y) { return; }
    let i = id.x + u.misc0.x * id.y;
    var sum = pixels[i].sum.xyz;
    var shown = pixels[i].shown.xyz;
    var albedo = pixels[i].albedo.xyz;
    var normal = pixels[i].normal.xyz;
    let batch = u.misc1.w;
    for (var s = 0u; s < batch; s++) {
        let sample_i = u.misc0.z + s;
        let sample = trace_sample(id.xy, sample_i);
        if (sample_i == 0u) {
            sum = sample.color;
            shown = sample.color;
            albedo = sample.albedo;
            normal = sample.normal;
        } else {
            sum = sum + sample.color;
            shown = display_of(sum, shown, sample.color, sample_i);
        }
    }
    pixels[i].sum = vec4<f32>(sum, 0.0);
    pixels[i].shown = vec4<f32>(shown, 0.0);
    pixels[i].albedo = vec4<f32>(albedo, 0.0);
    pixels[i].normal = vec4<f32>(normal, 0.0);
}

fn aces(x: f32) -> f32 {
    let c = max(x, 0.0);
    return clamp((c * (2.51 * c + 0.03)) / (c * (2.43 * c + 0.59) + 0.14), 0.0, 1.0);
}

fn tonemap(color: vec3<f32>) -> vec3<f32> {
    if ((u.misc1.x & 2u) != 0u) {
        return vec3<f32>(aces(color.x), aces(color.y), aces(color.z));
    }
    return clamp(color, vec3<f32>(0.0), vec3<f32>(1.0));
}

@compute @workgroup_size(8, 8)
fn display_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let width = i32(u.misc0.x);
    let height = i32(u.misc0.y);
    if (id.x >= u.misc0.x || id.y >= u.misc0.y) { return; }
    let x = i32(id.x);
    let y = i32(id.y);
    let i = id.x + u.misc0.x * id.y;
    var color = pixels[i].shown.xyz;
    if ((u.misc1.x & 4u) != 0u) {
        let base = pixels[i].albedo.xyz;
        var acc = vec3<f32>(0.0);
        var weight = 0.0;
        for (var dy = -1; dy <= 1; dy++) {
            for (var dx = -1; dx <= 1; dx++) {
                let nx = x + dx;
                let ny = y + dy;
                if (nx < 0 || ny < 0 || nx >= width || ny >= height) { continue; }
                let j = u32(nx) + u.misc0.x * u32(ny);
                let diff = base - pixels[j].albedo.xyz;
                let dist = sqrt(dot(diff, diff));
                let wgt = exp(-dist * 12.0);
                acc = acc + pixels[j].shown.xyz * wgt;
                weight = weight + wgt;
            }
        }
        color = acc / max(weight, 1e-6);
    }
    textureStore(display_tex, vec2<i32>(x, y), vec4<f32>(tonemap(color), 1.0));
}
