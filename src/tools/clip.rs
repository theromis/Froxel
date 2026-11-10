//! The CLIP semantic term of `optimize_to_views` (features `clip` and
//! `clip-metal`): patch-level similarity between a model's orthographic
//! views and its pixel-art views, from the vision tower of OpenAI's CLIP
//! ViT-B/32 (MIT licence) run with candle on Apple Metal or the CPU.
//!
//! An image is letterboxed onto 224 x 224 over a neutral grey, each pixel
//! art cell a block of pixels (nearest neighbour), and normalised with
//! CLIP's mean and deviation. The tokens of the second-to-last encoder
//! layer (spatial, unlike the pooled embedding) give one 768-vector per
//! 32 x 32 patch, 7 x 7 per image; a patch's similarity is the cosine of
//! the two images' vectors at it. The weights are a separate download
//! (`model.safetensors` of `openai/clip-vit-base-patch32`, about 605 MB),
//! read from `FROXEL_CLIP_MODEL` or `models/clip-vit-base-patch32.safetensors`
//! in Froxel's support folder, and loaded once per process.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use candle_core::{DType, Device, IndexOp, Tensor};
use candle_transformers::models::clip::vision_model::{ClipVisionConfig, ClipVisionTransformer};

use super::optimize::{PatchScorer, ViewImage, PATCH_GRID};

const SIZE: usize = 224;
const MEAN: [f32; 3] = [0.481_454_66, 0.457_827_5, 0.408_210_73];
const STD: [f32; 3] = [0.268_629_54, 0.261_302_6, 0.275_777_1];
/// Where the weights come from, for the error that says how to get them.
pub const CLIP_URL: &str =
    "https://huggingface.co/openai/clip-vit-base-patch32/resolve/refs%2Fpr%2F15/model.safetensors";

struct Loaded {
    path: PathBuf,
    model: ClipVisionTransformer,
    device: Device,
}

static MODEL: OnceLock<Mutex<Option<Loaded>>> = OnceLock::new();

/// The default weights file.
pub fn default_model() -> Option<PathBuf> {
    crate::lighting::support_file(
        "FROXEL_CLIP_MODEL",
        "models/clip-vit-base-patch32.safetensors",
    )
}

fn device() -> Device {
    #[cfg(feature = "clip-metal")]
    if let Ok(device) = Device::new_metal(0) {
        return device;
    }
    Device::Cpu
}

/// CLIP on the views, with the model at `path` (loaded once).
pub struct Clip {
    path: PathBuf,
    /// "metal" or "cpu".
    pub device: &'static str,
    /// Milliseconds the load took (0 when it was loaded already).
    pub load_ms: u64,
}

impl Clip {
    pub fn new(path: &Path) -> Result<Clip, String> {
        if !path.is_file() {
            return Err(format!(
                "semantic needs CLIP ViT-B/32 weights at {}: download model.safetensors (about 605 MB, MIT licence) from {CLIP_URL}, or pass semantic.model",
                path.display()
            ));
        }
        let started = std::time::Instant::now();
        let cell = MODEL.get_or_init(|| Mutex::new(None));
        let mut guard = cell.lock().map_err(|_| "the CLIP model lock is poisoned")?;
        let mut load_ms = 0;
        if guard.as_ref().is_none_or(|l| l.path != path) {
            let device = device();
            // SAFETY: the file is mapped read-only and not changed while
            // the process runs.
            let vb = unsafe {
                candle_nn::VarBuilder::from_mmaped_safetensors(&[path], DType::F32, &device)
            }
            .map_err(|err| format!("could not read {}: {err}", path.display()))?;
            let model = ClipVisionTransformer::new(
                vb.pp("vision_model"),
                &ClipVisionConfig::vit_base_patch32(),
            )
            .map_err(|err| format!("{} is not CLIP ViT-B/32: {err}", path.display()))?;
            *guard = Some(Loaded {
                path: path.to_path_buf(),
                model,
                device,
            });
            load_ms = started.elapsed().as_millis() as u64;
        }
        let name = match guard.as_ref().map(|l| &l.device) {
            Some(Device::Cpu) | None => "cpu",
            Some(_) => "metal",
        };
        Ok(Clip {
            path: path.to_path_buf(),
            device: name,
            load_ms,
        })
    }
}

/// `image` letterboxed onto 224 x 224 grey, CLIP-normalised, CHW.
fn pixels(image: &ViewImage) -> Vec<f32> {
    let (w, h) = (image.width.max(1), image.height.max(1));
    let scale = (SIZE as f32 / w as f32).min(SIZE as f32 / h as f32);
    let (dw, dh) = ((w as f32 * scale) as usize, (h as f32 * scale) as usize);
    let (ox, oy) = ((SIZE - dw) / 2, (SIZE - dh) / 2);
    let grey = [0.5f32; 3];
    let mut out = vec![0.0f32; 3 * SIZE * SIZE];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let rgb = if x >= ox && x < ox + dw && y >= oy && y < oy + dh {
                let sx = (((x - ox) as f32 / scale) as usize).min(w - 1);
                let sy = (((y - oy) as f32 / scale) as usize).min(h - 1);
                match image.pixels[sy * w + sx] {
                    Some(c) => c.map(|v| f32::from(v) / 255.0),
                    None => grey,
                }
            } else {
                grey
            };
            for c in 0..3 {
                out[c * SIZE * SIZE + y * SIZE + x] = (rgb[c] - MEAN[c]) / STD[c];
            }
        }
    }
    out
}

impl Clip {
    /// Unit patch vectors, `[image][patch][768]`.
    fn tokens(&self, images: &[&ViewImage]) -> Result<Vec<Vec<Vec<f32>>>, String> {
        let cell = MODEL.get_or_init(|| Mutex::new(None));
        let guard = cell.lock().map_err(|_| "the CLIP model lock is poisoned")?;
        let loaded = guard
            .as_ref()
            .filter(|l| l.path == self.path)
            .ok_or("the CLIP model is not loaded")?;
        let err = |e: candle_core::Error| format!("CLIP: {e}");
        let mut data = Vec::with_capacity(images.len() * 3 * SIZE * SIZE);
        for image in images {
            data.extend(pixels(image));
        }
        let input =
            Tensor::from_vec(data, (images.len(), 3, SIZE, SIZE), &loaded.device).map_err(err)?;
        let states = loaded.model.output_hidden_states(&input).map_err(err)?;
        // The last entry is the pooled output; the one before it the last
        // layer; the spatial layer is the one before that.
        let layer = &states[states.len().saturating_sub(3)];
        let patches = layer
            .i((.., 1.., ..))
            .map_err(err)?
            .to_dtype(DType::F32)
            .map_err(err)?
            .to_vec3::<f32>()
            .map_err(err)?;
        Ok(patches
            .into_iter()
            .map(|image| {
                image
                    .into_iter()
                    .map(|mut v| {
                        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
                        v.iter_mut().for_each(|x| *x /= n);
                        v
                    })
                    .collect()
            })
            .collect())
    }
}

impl PatchScorer for Clip {
    fn similarity(&mut self, pairs: &[(ViewImage, ViewImage)]) -> Result<Vec<Vec<f32>>, String> {
        let images: Vec<&ViewImage> = pairs.iter().flat_map(|(a, b)| [a, b]).collect();
        let tokens = self.tokens(&images)?;
        Ok(tokens
            .chunks(2)
            .map(|pair| {
                (0..PATCH_GRID * PATCH_GRID)
                    .map(|k| {
                        pair[0][k]
                            .iter()
                            .zip(&pair[1][k])
                            .map(|(a, b)| a * b)
                            .sum::<f32>()
                    })
                    .collect()
            })
            .collect())
    }

    fn name(&self) -> String {
        format!("clip-vit-b32 ({})", self.device)
    }
}
