//! Finished-frame denoiser.
//!
//! The default build keeps the tracer's edge-aware filter. The `oidn` feature
//! replaces that filter with Open Image Denoise, guided by the first-hit
//! albedo and normal, on both the CPU frame and the GPU readback.

use crate::trace::Image;

pub fn available() -> bool {
    cfg!(feature = "oidn")
}

pub fn name() -> &'static str {
    if available() {
        "Open Image Denoise"
    } else {
        "edge filter"
    }
}

pub fn hover() -> &'static str {
    if available() {
        "Open Image Denoise on the finished frame, using the first-hit albedo and normal. The viewport switches to it when Samples is reached."
    } else {
        "Edge-aware filter on the finished frame. Build with --features oidn to link Open Image Denoise."
    }
}

pub fn denoise_image(image: &Image) -> Result<Vec<[f32; 3]>, String> {
    let mut color = image.mean();
    denoise_buffers(
        &mut color,
        image.albedo(),
        image.normal(),
        image.width,
        image.height,
    )?;
    Ok(color)
}

pub fn denoise_buffers(
    color: &mut [[f32; 3]],
    albedo: &[[f32; 3]],
    normal: &[[f32; 3]],
    width: u32,
    height: u32,
) -> Result<(), String> {
    let count = (width as usize).saturating_mul(height as usize);
    if count == 0 || color.len() != count || albedo.len() != count || normal.len() != count {
        return Err("denoiser guides do not match the frame".into());
    }
    #[cfg(feature = "oidn")]
    {
        oidn_denoise(color, albedo, normal, width, height)
    }
    #[cfg(not(feature = "oidn"))]
    {
        let _ = (color, albedo, normal, width, height);
        Err("Open Image Denoise is not linked".into())
    }
}

#[cfg(feature = "oidn")]
fn oidn_denoise(
    color: &mut [[f32; 3]],
    albedo: &[[f32; 3]],
    normal: &[[f32; 3]],
    width: u32,
    height: u32,
) -> Result<(), String> {
    let flat = |src: &[[f32; 3]]| {
        let mut out = Vec::with_capacity(src.len() * 3);
        for pixel in src {
            out.extend_from_slice(pixel);
        }
        out
    };
    let beauty = flat(color);
    let albedo = flat(albedo);
    let normal = flat(normal);
    let device = oidn::Device::new().map_err(|err| format!("Open Image Denoise: {err}"))?;
    let mut output = vec![0.0f32; beauty.len()];
    oidn::RayTracing::try_new(&device)
        .map_err(|err| format!("Open Image Denoise: {err}"))?
        .image_dimensions(width as usize, height as usize)
        .albedo_normal(&albedo, &normal)
        .clean_aux(true)
        .hdr(true)
        .srgb(false)
        .filter(&beauty, &mut output)
        .map_err(|err| format!("Open Image Denoise: {err}"))?;
    for (pixel, chunk) in color.iter_mut().zip(output.chunks_exact(3)) {
        *pixel = [chunk[0], chunk[1], chunk[2]];
    }
    Ok(())
}
