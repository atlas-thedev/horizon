// ── Native Motion Blur (tools/native/native_tweaks.py appends this to gfx/world_post.rs) ──────────────────
// The Motion Blur module hands its strength to `tweaks::motion_blur`. The world then always renders into this pass's
// own target, and at the copy to the screen each frame is blended into a history that keeps `strength` of the last
// one per 1/60 s. The hand and HUD draw after it, so they stay sharp.

struct NativeBlur {
    size: [u32; 2],
    format: wgpu::TextureFormat,
    view: wgpu::TextureView,
    show: wgpu::BindGroup,
    blend: wgpu::RenderPipeline,
    last: Option<std::time::Instant>,
}

#[derive(Default)]
struct NativeBlurState {
    device: Option<wgpu::Device>,
    size: [u32; 2],
    blur: Option<NativeBlur>,
}

fn native_blur_state() -> std::sync::MutexGuard<'static, NativeBlurState> {
    static STATE: std::sync::OnceLock<std::sync::Mutex<NativeBlurState>> = std::sync::OnceLock::new();
    STATE.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// How much of the history stays after `seconds`: `keep` per 1/60 s, so a trail lasts as long at any frame rate. After
/// a stall (a hitch, a window brought back) nothing stays: the old view would linger as a ghost.
fn native_blur_stay(keep: f32, seconds: f64) -> f64 {
    if !(0.0..=0.25).contains(&seconds) {
        return 0.0;
    }
    f64::from(keep.clamp(0.0, 0.95)).powf(seconds * 60.0)
}

/// Whether Motion Blur is on this frame; remembers the device and the screen size for `native_blur`.
fn native_blur_ready(device: &wgpu::Device, window: [u32; 2]) -> bool {
    let on = crate::native_ui::tweaks::get().motion_blur.is_some_and(|k| k > 0.0);
    let mut s = native_blur_state();
    if on {
        s.device = Some(device.clone());
        s.size = window;
    } else {
        s.blur = None;
        s.device = None;
    }
    on
}

impl NativeBlur {
    fn new(device: &wgpu::Device, post: &Post, size: [u32; 2]) -> Option<Self> {
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let view = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("Native motion blur"),
                size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: post.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Native motion blur"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../world_post.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Native motion blur"),
            bind_group_layouts: &[Some(&post.layout)],
            ..Default::default()
        });
        // new = frame * constant + history * (1 - constant)
        let mix = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Constant,
            dst_factor: wgpu::BlendFactor::OneMinusConstant,
            operation: wgpu::BlendOperation::Add,
        };
        let blend = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Native motion blur"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_post"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_copy"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: post.format,
                    blend: Some(wgpu::BlendState { color: mix, alpha: mix }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let show = post.bind(device, &view);
        let error = pollster::block_on(memory.pop()).or(pollster::block_on(validation.pop()));
        if let Some(e) = error {
            log::warn!("Native renderer: motion blur unavailable ({e})");
            return None;
        }
        Some(Self { size, format: post.format, view, show, blend, last: None })
    }
}

/// Blends `group` (this frame's world) into the history and returns the history's binding to show instead.
fn native_blur(post: &Post, encoder: &mut wgpu::CommandEncoder, group: &wgpu::BindGroup) -> Option<wgpu::BindGroup> {
    let keep = crate::native_ui::tweaks::get().motion_blur.filter(|k| *k > 0.0)?;
    let mut s = native_blur_state();
    let device = s.device.clone()?;
    let size = s.size;
    if size[0] == 0 || size[1] == 0 {
        return None;
    }
    if s.blur.as_ref().is_none_or(|b| b.size != size || b.format != post.format) {
        s.blur = NativeBlur::new(&device, post, size);
    }
    let b = s.blur.as_mut()?;
    let now = std::time::Instant::now();
    // the first frame (or one after a long pause) fills the history with this frame alone
    let stay = b.last.map_or(0.0, |t| native_blur_stay(keep, now.duration_since(t).as_secs_f64()));
    b.last = Some(now);
    let attachments = [Some(wgpu::RenderPassColorAttachment {
        view: &b.view,
        resolve_target: None,
        depth_slice: None,
        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
    })];
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Native motion blur"),
        color_attachments: &attachments,
        ..Default::default()
    });
    pass.set_pipeline(&b.blend);
    let fresh = 1.0 - stay;
    pass.set_blend_constant(wgpu::Color { r: fresh, g: fresh, b: fresh, a: fresh });
    pass.set_bind_group(0, group, &[]);
    pass.draw(0..3, 0..1);
    drop(pass);
    Some(b.show.clone())
}

impl Post {
    /// `finish_with`, through Motion Blur when it is on.
    fn native_finish(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        screen: &wgpu::TextureView,
        group: &wgpu::BindGroup,
        timing: Option<&mut super::pack_timing::PassTiming>,
    ) {
        let blurred = native_blur(self, encoder, group);
        self.finish_with(encoder, screen, blurred.as_ref().unwrap_or(group), timing);
    }
}

#[cfg(test)]
mod native_blur_tests {
    #[test]
    fn the_trail_lasts_as_long_at_any_frame_rate() {
        let at60 = super::native_blur_stay(0.5, 1.0 / 60.0);
        let at120_twice = super::native_blur_stay(0.5, 1.0 / 120.0).powi(2);
        assert!((at60 - 0.5).abs() < 1e-9 && (at120_twice - 0.5).abs() < 1e-9);
        assert_eq!(super::native_blur_stay(0.9, 1.0), 0.0, "a stall clears the trail");
        assert!((super::native_blur_stay(0.99, 1.0 / 60.0) - 0.95).abs() < 1e-6, "strength is capped");
    }
}
