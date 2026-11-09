//! Texture images for mesh import: PNG, JPEG, and WebP decoding, and the base64 of
//! glTF `data:` URIs.

use super::{Texture, Wrap};

/// The largest texture side import decodes.
const MAX_SIDE: u32 = 16_384;

/// Decode a PNG, JPEG, or WebP by its first bytes into sRGB RGBA.
pub(crate) fn decode(bytes: &[u8], what: &str) -> Result<Texture, String> {
    if bytes.starts_with(b"\x89PNG") {
        decode_png(bytes, what)
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        decode_jpeg(bytes, what)
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        decode_webp(bytes, what)
    } else if bytes.starts_with(b"\xabKTX 20\xbb") {
        Err(format!(
            "{what} is KTX2 (Basis), which import does not read; use PNG, JPEG, or WebP"
        ))
    } else {
        Err(format!("{what} is not a PNG, JPEG, or WebP"))
    }
}

fn decode_png(bytes: &[u8], what: &str) -> Result<Texture, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder
        .read_info()
        .map_err(|err| format!("{what} is not a PNG: {err}"))?;
    let (width, height) = reader.info().size();
    check_size(width, height, what)?;
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| format!("{what} is too large"))?;
    let mut buf = vec![0; size];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|err| format!("Could not read {what}: {err}"))?;
    let stride = info.line_size;
    let mut rgba = Vec::with_capacity((info.width * info.height) as usize);
    for row in 0..info.height as usize {
        let line = &buf[row * stride..row * stride + stride];
        for x in 0..info.width as usize {
            rgba.push(match info.color_type {
                png::ColorType::Rgba => [
                    line[x * 4],
                    line[x * 4 + 1],
                    line[x * 4 + 2],
                    line[x * 4 + 3],
                ],
                png::ColorType::Rgb => [line[x * 3], line[x * 3 + 1], line[x * 3 + 2], 255],
                png::ColorType::GrayscaleAlpha => {
                    [line[x * 2], line[x * 2], line[x * 2], line[x * 2 + 1]]
                }
                png::ColorType::Grayscale => [line[x], line[x], line[x], 255],
                png::ColorType::Indexed => return Err(format!("{what} kept its PNG palette")),
            });
        }
    }
    Ok(Texture {
        width: info.width,
        height: info.height,
        rgba,
        wrap: [Wrap::Repeat; 2],
    })
}

fn decode_jpeg(bytes: &[u8], what: &str) -> Result<Texture, String> {
    use zune_jpeg::zune_core::colorspace::ColorSpace;
    use zune_jpeg::zune_core::options::DecoderOptions;
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut decoder =
        zune_jpeg::JpegDecoder::new_with_options(std::io::Cursor::new(bytes), options);
    decoder
        .decode_headers()
        .map_err(|err| format!("{what} is not a JPEG: {err}"))?;
    let (width, height) = decoder
        .dimensions()
        .ok_or_else(|| format!("{what} has no JPEG size"))?;
    check_size(width as u32, height as u32, what)?;
    let pixels = decoder
        .decode()
        .map_err(|err| format!("Could not read {what}: {err}"))?;
    let channels = match decoder.output_colorspace() {
        Some(ColorSpace::Luma) => 1,
        _ => 3,
    };
    if pixels.len() < width * height * channels {
        return Err(format!("{what} decoded short"));
    }
    let rgba = pixels
        .chunks_exact(channels)
        .take(width * height)
        .map(|px| match px {
            [l] => [*l, *l, *l, 255],
            _ => [px[0], px[1], px[2], 255],
        })
        .collect();
    Ok(Texture {
        width: width as u32,
        height: height as u32,
        rgba,
        wrap: [Wrap::Repeat; 2],
    })
}

fn decode_webp(bytes: &[u8], what: &str) -> Result<Texture, String> {
    let mut decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(bytes))
        .map_err(|err| format!("{what} is not a WebP: {err}"))?;
    let (width, height) = decoder.dimensions();
    check_size(width, height, what)?;
    let alpha = decoder.has_alpha();
    let size = decoder
        .output_buffer_size()
        .ok_or_else(|| format!("{what} is too large"))?;
    let mut buf = vec![0; size];
    // An animated WebP reads its first frame.
    decoder
        .read_image(&mut buf)
        .map_err(|err| format!("Could not read {what}: {err}"))?;
    let channels = if alpha { 4 } else { 3 };
    let rgba = buf
        .chunks_exact(channels)
        .take((width * height) as usize)
        .map(|px| [px[0], px[1], px[2], if alpha { px[3] } else { 255 }])
        .collect();
    Ok(Texture {
        width,
        height,
        rgba,
        wrap: [Wrap::Repeat; 2],
    })
}

fn check_size(width: u32, height: u32, what: &str) -> Result<(), String> {
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(format!(
            "{what} is {width}x{height}; a texture is 1 to {MAX_SIDE} pixels a side"
        ));
    }
    Ok(())
}

/// Standard base64, with or without padding; whitespace is skipped.
pub(crate) fn base64(text: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return Err("data URI is not valid base64".into()),
        };
        acc = (acc << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

/// `%20` and the other percent escapes of a relative URI.
pub(crate) fn percent_decode(uri: &str) -> String {
    let bytes = uri.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(value) = u8::from_str_radix(hex, 16) {
                out.push(value);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
