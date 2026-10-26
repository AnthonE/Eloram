//! Mipmaps for every image loaded from a file. Neither PNG nor JPEG carries them, and Bevy's own
//! generator is a compute shader WebGL2 doesn't have; without them a tiled floor seen at a slant
//! shimmers. So each image that comes in from a file (the ground's arrays, the models' textures,
//! the brush) gets its chain here, on the CPU, once: box-filtered, in linear light where the
//! texture is sRGB, every array layer its own. Images made in code (the fog, the splat) are left
//! alone: they change, or are smooth already.

use bevy::asset::AssetEvent;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{TextureDataOrder, TextureDimension, TextureFormat};

pub struct MipsPlugin;

impl Plugin for MipsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, add_mips);
    }
}

/// A sampler that uses the chain: linear throughout, anisotropic, keeping the address modes asked.
pub fn sampler(repeat: bool) -> ImageSampler {
    let mode = if repeat { ImageAddressMode::Repeat } else { ImageAddressMode::ClampToEdge };
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: mode,
        address_mode_v: mode,
        address_mode_w: mode,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    })
}

fn add_mips(
    mut events: MessageReader<AssetEvent<Image>>,
    server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
) {
    for e in events.read() {
        let AssetEvent::Added { id } = *e else { continue };
        if server.get_path(id).is_none() {
            continue;
        }
        let Some(image) = images.get(id) else { continue };
        let d = &image.texture_descriptor;
        let srgb = match d.format {
            TextureFormat::Rgba8UnormSrgb => true,
            TextureFormat::Rgba8Unorm => false,
            _ => continue,
        };
        if d.mip_level_count != 1 || d.dimension != TextureDimension::D2 || image.data.is_none() {
            continue;
        }
        let Some(mut image) = images.get_mut(id) else { continue };
        let (w, h, layers) = {
            let s = image.texture_descriptor.size;
            (s.width as usize, s.height as usize, s.depth_or_array_layers as usize)
        };
        let Some(data) = image.data.take() else { continue };
        let (chain, levels) = build(&data, w, h, layers, srgb);
        image.data = Some(chain);
        image.data_order = TextureDataOrder::LayerMajor;
        image.texture_descriptor.mip_level_count = levels;
        image.sampler = match &image.sampler {
            ImageSampler::Descriptor(d) => ImageSampler::Descriptor(ImageSamplerDescriptor {
                mag_filter: ImageFilterMode::Linear,
                min_filter: ImageFilterMode::Linear,
                mipmap_filter: ImageFilterMode::Linear,
                anisotropy_clamp: 8,
                ..d.clone()
            }),
            ImageSampler::Default => sampler(true),
        };
    }
}

/// Every layer's chain, layer after layer, and how many levels each has.
fn build(data: &[u8], w: usize, h: usize, layers: usize, srgb: bool) -> (Vec<u8>, u32) {
    let levels = (w.max(h) as f32).log2().floor() as u32 + 1;
    let to_lin: Vec<f32> = (0..256)
        .map(|v| {
            let c = v as f32 / 255.0;
            if srgb { if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) } } else { c }
        })
        .collect();
    let to_byte = |c: f32| -> u8 {
        let c = c.clamp(0.0, 1.0);
        let e = if srgb {
            if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
        } else {
            c
        };
        (e * 255.0 + 0.5) as u8
    };
    let mut out = Vec::with_capacity(data.len() * 4 / 3 + 64);
    for layer in data.chunks_exact(w * h * 4).take(layers) {
        out.extend_from_slice(layer);
        let mut cur: Vec<f32> = layer
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| {
                [to_lin[p[0] as usize], to_lin[p[1] as usize], to_lin[p[2] as usize], p[3] as f32 / 255.0]
            })
            .collect();
        let (mut cw, mut ch) = (w, h);
        for _ in 1..levels {
            let (nw, nh) = ((cw / 2).max(1), (ch / 2).max(1));
            let mut next = vec![0.0f32; nw * nh * 4];
            for y in 0..nh {
                for x in 0..nw {
                    let (x0, y0) = ((2 * x).min(cw - 1), (2 * y).min(ch - 1));
                    let (x1, y1) = ((2 * x + 1).min(cw - 1), (2 * y + 1).min(ch - 1));
                    for c in 0..4 {
                        let s = cur[(y0 * cw + x0) * 4 + c]
                            + cur[(y0 * cw + x1) * 4 + c]
                            + cur[(y1 * cw + x0) * 4 + c]
                            + cur[(y1 * cw + x1) * 4 + c];
                        next[(y * nw + x) * 4 + c] = s * 0.25;
                    }
                }
            }
            for p in next.as_chunks::<4>().0 {
                out.extend_from_slice(&[
                    to_byte(p[0]),
                    to_byte(p[1]),
                    to_byte(p[2]),
                    (p[3] * 255.0 + 0.5) as u8,
                ]);
            }
            cur = next;
            (cw, ch) = (nw, nh);
        }
    }
    (out, levels)
}
