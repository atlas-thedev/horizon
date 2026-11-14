// Native Client UI texture pages for the HUD pass. `tools/native/native_ui.py` includes this file at
// the end of `hud.rs`, so it sees the renderer's private fields.
//
// The Native UI (`native_ui`) draws from its own textures, as native-mod's Canvas does: page 0 is its
// atlas (glyphs, rounded shapes, icons, faces), the other pages are whole images (backgrounds, covers,
// screenshots). A run of HUD vertices drawn from page `n` is marked `Blend::Native(n)` and drawn with
// the HUD's own pipeline and shader, with a bind group whose texture is that page.

/// New pixels for a Native UI page this frame: the page is `size` texels (made, or made again when its
/// size changed), and `rect` (x, y, w, h) of it gets `rgba`. An empty `size` drops the page.
#[derive(Clone, Debug, Default)]
pub struct NativeUpload {
    pub page: u16,
    pub size: [u32; 2],
    pub rect: [u32; 4],
    pub rgba: Vec<u8>,
    /// Sampled with linear filtering (images); the atlas is sampled texel for texel.
    pub smooth: bool,
}

/// Set when the renderer lost (or never had) what the Native UI uploaded before: a new renderer, a page made
/// again for part of its pixels, a page drawn that is not there. The UI then uploads everything again.
pub static NATIVE_PAGES_LOST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

struct NativePage {
    texture: wgpu::Texture,
    group: wgpu::BindGroup,
    smooth: bool,
}

/// The pages the renderer holds, and what it makes their bind groups from.
pub struct NativePages {
    pages: Vec<Option<NativePage>>,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    nearest: wgpu::Sampler,
    linear: wgpu::Sampler,
    glint_view: wgpu::TextureView,
    glint_sampler: wgpu::Sampler,
    /// Binding 6 of the HUD layout since Nightly 2026-11-13 (the armour glint), when the layout has it.
    glint_armor_view: Option<wgpu::TextureView>,
}

impl NativePages {
    fn new(
        layout: &wgpu::BindGroupLayout,
        uniform: &wgpu::Buffer,
        nearest: &wgpu::Sampler,
        linear: &wgpu::Sampler,
        glint_view: &wgpu::TextureView,
        glint_sampler: &wgpu::Sampler,
        glint_armor_view: Option<&wgpu::TextureView>,
    ) -> Self {
        NATIVE_PAGES_LOST.store(true, std::sync::atomic::Ordering::Relaxed);
        Self {
            pages: Vec::new(),
            layout: layout.clone(),
            uniform: uniform.clone(),
            nearest: nearest.clone(),
            linear: linear.clone(),
            glint_view: glint_view.clone(),
            glint_sampler: glint_sampler.clone(),
            glint_armor_view: glint_armor_view.cloned(),
        }
    }

    fn group(&self, page: u16) -> Option<&wgpu::BindGroup> {
        self.pages.get(page as usize)?.as_ref().map(|p| &p.group)
    }

    fn make(&self, device: &wgpu::Device, size: [u32; 2], smooth: bool) -> NativePage {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Native UI page"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut entries = vec![
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(if smooth {
                        &self.linear
                    } else {
                        &self.nearest
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&self.glint_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&self.glint_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(&self.linear),
                },
        ];
        // the group must name every binding of the HUD layout: 2026-11-13 added the armour glint as binding 6
        if let Some(armor) = &self.glint_armor_view {
            entries.push(wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(armor),
            });
        }
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Native UI page"),
            layout: &self.layout,
            entries: &entries,
        });
        NativePage {
            texture,
            group,
            smooth,
        }
    }

    fn upload(
        &mut self,
        device: &wgpu::Device,
        uploads: &mut crate::gfx::upload::Uploads,
        encoder: &mut wgpu::CommandEncoder,
        list: &[NativeUpload],
    ) {
        for u in list {
            let i = u.page as usize;
            if self.pages.len() <= i {
                self.pages.resize_with(i + 1, || None);
            }
            if u.size.contains(&0) {
                self.pages[i] = None;
                continue;
            }
            let fits = self.pages[i].as_ref().is_some_and(|p| {
                p.texture.width() == u.size[0] && p.texture.height() == u.size[1] && p.smooth == u.smooth
            });
            if !fits {
                if u.rect != [0, 0, u.size[0], u.size[1]] {
                    // Only part of a page that is not here: the rest has to come again.
                    NATIVE_PAGES_LOST.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                self.pages[i] = Some(self.make(device, u.size, u.smooth));
            }
            let [x, y, w, h] = u.rect;
            let inside = x.saturating_add(w) <= u.size[0] && y.saturating_add(h) <= u.size[1];
            if w == 0 || h == 0 || !inside || u.rgba.len() < (w * h * 4) as usize {
                continue;
            }
            let Some(page) = &self.pages[i] else { continue };
            uploads.texture(
                encoder,
                wgpu::TexelCopyTextureInfo {
                    texture: &page.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                w,
                h,
                &u.rgba,
            );
        }
    }
}
