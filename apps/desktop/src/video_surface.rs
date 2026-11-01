use iced::widget::shader::{self, Primitive, Program};
use iced::{Rectangle, mouse};

pub fn fit_factors(source_width: u32, source_height: u32, target: Rectangle) -> (f32, f32) {
    if source_width == 0 || source_height == 0 || target.width <= 0.0 || target.height <= 0.0 {
        return (1.0, 1.0);
    }
    let source_aspect = source_width as f32 / source_height as f32;
    let target_aspect = target.width / target.height;
    if source_aspect > target_aspect {
        (1.0, target_aspect / source_aspect)
    } else {
        (source_aspect / target_aspect, 1.0)
    }
}

pub fn source_uv(target: (f32, f32), fit: (f32, f32)) -> Option<(f32, f32)> {
    if fit.0 <= 0.0 || fit.1 <= 0.0 {
        return None;
    }
    let u = (target.0 - 0.5) / fit.0 + 0.5;
    let v = (target.1 - 0.5) / fit.1 + 0.5;
    ((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)).then_some((u, v))
}

pub use screenroom_player::Frame;

#[derive(Debug, Clone, Default)]
pub struct VideoSurface {
    frame: Option<Frame>,
}

impl VideoSurface {
    pub fn new(frame: Option<Frame>) -> Self {
        Self { frame }
    }
}

#[derive(Debug)]
pub struct VideoPrimitive {
    frame: Option<Frame>,
    bounds: Rectangle,
}

pub fn surface_key(frame: &Frame) -> Option<usize> {
    #[cfg(target_os = "macos")]
    {
        frame
            .surface
            .as_ref()
            .map(|surface| surface.as_ptr() as usize)
            .filter(|key| *key != 0)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = frame;
        None
    }
}

const MAX_IMPORTED_SURFACES: usize = 12;

pub fn frame_tag(frame: &Frame) -> usize {
    if let Some(key) = surface_key(frame) {
        return key;
    }
    std::sync::Arc::as_ptr(&frame.rgba) as usize
}

pub fn holds_a_gpu_surface(frame: &Frame) -> bool {
    surface_key(frame).is_some()
}

impl Program<crate::app::Message> for VideoSurface {
    type State = ();
    type Primitive = VideoPrimitive;

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: mouse::Cursor,
        bounds: Rectangle,
    ) -> Self::Primitive {
        VideoPrimitive {
            frame: self.frame.clone(),
            bounds,
        }
    }
}

impl Primitive for VideoPrimitive {
    type Pipeline = VideoPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _bounds: &Rectangle,
        _viewport: &shader::Viewport,
    ) {
        let Some(frame) = self.frame.as_ref().filter(|frame| frame.is_well_formed()) else {
            return;
        };
        let (fit_x, fit_y) = fit_factors(frame.width, frame.height, self.bounds);
        queue.write_buffer(
            &pipeline.fit_buffer,
            0,
            bytemuck_f32(&[fit_x, fit_y, 0.0, 0.0]),
        );
        if pipeline.uploaded_generation == Some(frame.generation)
            && pipeline.texture_size == (frame.width, frame.height)
            && pipeline.uploaded_surface == Some(frame_tag(frame))
        {
            return;
        }

        #[cfg(target_os = "macos")]
        if let Some(surface) = frame.surface.clone() {
            if pipeline.imported_size != (frame.width, frame.height)
                || pipeline.imported.len() >= MAX_IMPORTED_SURFACES
            {
                pipeline.imported.clear();
                pipeline.imported_size = (frame.width, frame.height);
            }
            let key = surface.as_ptr() as usize;
            if !pipeline.imported.contains_key(&key) {
                let Some(imported) = import_surface(device, pipeline, &surface, frame) else {
                    return;
                };
                pipeline.imported.insert(key, imported);
            }
            if let Some(imported) = pipeline.imported.get(&key) {
                pipeline.bind_group = Some(imported.bind_group.clone());
            }
            crate::play_check::note_import();
            pipeline.texture_size = (frame.width, frame.height);
            pipeline.uploaded_generation = Some(frame.generation);
            pipeline.uploaded_surface = Some(key);
            return;
        }

        if pipeline.texture_size != (frame.width, frame.height) {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("screenroom-video-frame"),
                size: wgpu::Extent3d {
                    width: frame.width,
                    height: frame.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            pipeline.cpu_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("screenroom-video-frame"),
                layout: &pipeline.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&pipeline.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: pipeline.fit_buffer.as_entire_binding(),
                    },
                ],
            }));
            pipeline.texture = Some(texture);
            pipeline.texture_size = (frame.width, frame.height);
        }
        pipeline.bind_cpu_group();

        if let Some(texture) = &pipeline.texture {
            crate::play_check::note_upload();
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &frame.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: frame.offset as u64,
                    bytes_per_row: Some(frame.stride),
                    rows_per_image: Some(frame.height),
                },
                wgpu::Extent3d {
                    width: frame.width,
                    height: frame.height,
                    depth_or_array_layers: 1,
                },
            );
        }

        pipeline.uploaded_generation = Some(frame.generation);
        pipeline.uploaded_surface = Some(frame_tag(frame));
    }

    fn draw(&self, pipeline: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        let Some(bind_group) = &pipeline.bind_group else {
            return false;
        };
        render_pass.set_pipeline(&pipeline.render_pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        render_pass.draw(0..3, 0..1);
        crate::play_check::note_draw();
        true
    }
}

pub struct VideoPipeline {
    render_pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    fit_buffer: wgpu::Buffer,
    bind_group: Option<wgpu::BindGroup>,
    texture: Option<wgpu::Texture>,
    cpu_bind_group: Option<wgpu::BindGroup>,
    texture_size: (u32, u32),
    uploaded_generation: Option<u64>,
    uploaded_surface: Option<usize>,
    #[cfg(target_os = "macos")]
    imported: std::collections::HashMap<usize, ImportedSurface>,
    #[cfg(target_os = "macos")]
    imported_size: (u32, u32),
}

#[cfg(target_os = "macos")]
struct ImportedSurface {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

#[cfg(target_os = "macos")]
impl std::fmt::Debug for ImportedSurface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImportedSurface")
            .field("size", &(self.texture.width(), self.texture.height()))
            .finish_non_exhaustive()
    }
}

impl VideoPipeline {
    fn bind_cpu_group(&mut self) {
        self.bind_group = self.cpu_bind_group.clone();
    }
}

#[cfg(target_os = "macos")]
fn import_surface(
    device: &wgpu::Device,
    pipeline: &VideoPipeline,
    surface: &screenroom_player::SurfaceFrame,
    frame: &Frame,
) -> Option<ImportedSurface> {
    let texture = macos::texture_from_surface(device, surface, frame)?;
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("screenroom-video-surface-imported"),
        layout: &pipeline.layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&pipeline.sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: pipeline.fit_buffer.as_entire_binding(),
            },
        ],
    });
    Some(ImportedSurface {
        texture,
        bind_group,
    })
}

#[cfg(target_os = "macos")]
mod macos {
    use super::Frame;
    use metal::foreign_types::ForeignType;
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use screenroom_player::{PixelFormat, SurfaceFrame};

    fn texture_formats(format: PixelFormat) -> (metal::MTLPixelFormat, wgpu::TextureFormat) {
        match format {
            PixelFormat::Bgra8 => (
                metal::MTLPixelFormat::BGRA8Unorm_sRGB,
                wgpu::TextureFormat::Bgra8UnormSrgb,
            ),
            PixelFormat::Rgba8 => (
                metal::MTLPixelFormat::RGBA8Unorm_sRGB,
                wgpu::TextureFormat::Rgba8UnormSrgb,
            ),
            PixelFormat::Rgba16Float => (
                metal::MTLPixelFormat::RGBA16Float,
                wgpu::TextureFormat::Rgba16Float,
            ),
        }
    }

    pub fn texture_from_surface(
        device: &wgpu::Device,
        surface: &SurfaceFrame,
        frame: &Frame,
    ) -> Option<wgpu::Texture> {
        let width = frame.width;
        let height = frame.height;
        if width == 0 || height == 0 {
            return None;
        }

        let hal_device = unsafe { device.as_hal::<wgpu::hal::api::Metal>() }?;
        let device_pointer: *mut AnyObject = {
            let guarded = hal_device.raw_device();
            let metal_device = guarded.lock();
            metal_device.as_ptr().cast::<AnyObject>()
        };
        if device_pointer.is_null() {
            return None;
        }

        let (metal_format, wgpu_format) = texture_formats(surface.format());

        let descriptor = metal::TextureDescriptor::new();
        descriptor.set_width(width as u64);
        descriptor.set_height(height as u64);
        descriptor.set_pixel_format(metal_format);
        descriptor.set_texture_type(metal::MTLTextureType::D2);
        descriptor.set_usage(metal::MTLTextureUsage::ShaderRead);
        descriptor.set_storage_mode(metal::MTLStorageMode::Shared);

        let descriptor_object: *mut AnyObject = descriptor.as_ptr().cast::<AnyObject>();
        let surface_object: *mut AnyObject = surface.as_ptr().cast::<AnyObject>();

        let raw: *mut AnyObject = unsafe {
            msg_send![
                device_pointer,
                newTextureWithDescriptor: descriptor_object,
                iosurface: surface_object,
                plane: 0usize
            ]
        };
        if raw.is_null() {
            return None;
        }
        let texture = unsafe { metal::Texture::from_ptr(raw.cast()) };

        let hal_texture = unsafe {
            wgpu::hal::metal::Device::texture_from_raw(
                texture,
                wgpu_format,
                metal::MTLTextureType::D2,
                1,
                1,
                wgpu::hal::CopyExtent {
                    width,
                    height,
                    depth: 1,
                },
            )
        };

        let description = wgpu::TextureDescriptor {
            label: Some("screenroom-video-surface-imported"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu_format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };

        Some(unsafe {
            device.create_texture_from_hal::<wgpu::hal::api::Metal>(hal_texture, &description)
        })
    }
}

impl std::fmt::Debug for VideoPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoPipeline")
            .field("texture_size", &self.texture_size)
            .field("uploaded_generation", &self.uploaded_generation)
            .finish_non_exhaustive()
    }
}

impl shader::Pipeline for VideoPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("screenroom-video-surface"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("screenroom-video-surface"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("screenroom-video-surface"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("screenroom-video-surface"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("screenroom-video-surface"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let fit_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screenroom-video-fit"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            render_pipeline,
            layout,
            sampler,
            fit_buffer,
            bind_group: None,
            texture: None,
            cpu_bind_group: None,
            texture_size: (0, 0),
            uploaded_generation: None,
            uploaded_surface: None,
            #[cfg(target_os = "macos")]
            imported: std::collections::HashMap::new(),
            #[cfg(target_os = "macos")]
            imported_size: (0, 0),
        }
    }

    fn trim(&mut self) {
        #[cfg(target_os = "macos")]
        self.imported.clear();
        self.bind_group = self.cpu_bind_group.clone();
    }
}

fn bytemuck_f32(values: &[f32; 4]) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(values.as_ptr().cast::<u8>(), std::mem::size_of_val(values))
    }
}

const SHADER: &str = r#"
struct Fit {
    fit: vec2<f32>,
    padding: vec2<f32>,
};

@group(0) @binding(0) var frame_texture: texture_2d<f32>;
@group(0) @binding(1) var frame_sampler: sampler;
@group(0) @binding(2) var<uniform> fit: Fit;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    // One oversized triangle covering the widget bounds.
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    var uvs = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 2.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(2.0, 0.0),
    );
    var out: VertexOutput;
    out.position = vec4<f32>(positions[index], 0.0, 1.0);
    out.uv = uvs[index];
    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // `fit` is the fraction of each axis the video occupies, so the source
    // coordinate is found by dividing: pixels outside 0..1 are the bars and are
    // drawn black. Multiplying here would stretch the picture instead.
    var uv = (input.uv - vec2<f32>(0.5, 0.5)) / fit.fit + vec2<f32>(0.5, 0.5);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    // libmpv leaves the frame's fourth byte undefined, so opacity is forced here
    // rather than trusted from the buffer. Doing it on the GPU also keeps the
    // host-side frame path free of per-pixel work.
    return vec4<f32>(textureSample(frame_texture, frame_sampler, uv).rgb, 1.0);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(width: f32, height: f32) -> Rectangle {
        Rectangle {
            x: 0.0,
            y: 0.0,
            width,
            height,
        }
    }

    #[test]
    fn letterboxes_wider_source_into_square() {
        let (x, y) = fit_factors(1920, 800, bounds(500.0, 500.0));
        assert_eq!(x, 1.0);
        assert!((y - 500.0 / 1200.0).abs() < 1e-6);
    }

    #[test]
    fn pillarboxes_taller_source_into_wide() {
        let (x, y) = fit_factors(800, 1200, bounds(1000.0, 500.0));
        assert!(x < 1.0);
        assert_eq!(y, 1.0);
    }

    #[test]
    fn matching_aspect_has_no_bars() {
        let (x, y) = fit_factors(1920, 1080, bounds(1280.0, 720.0));
        assert!((x - 1.0).abs() < 1e-6);
        assert!((y - 1.0).abs() < 1e-6);
    }

    #[test]
    fn degenerate_inputs_do_not_panic_or_stretch() {
        assert_eq!(fit_factors(0, 0, bounds(100.0, 100.0)), (1.0, 1.0));
        assert_eq!(fit_factors(100, 100, bounds(0.0, 100.0)), (1.0, 1.0));
    }

    #[test]
    fn the_centre_of_the_widget_is_the_centre_of_the_frame() {
        let fit = fit_factors(1920, 1080, bounds(1240.0, 425.0));
        let (u, v) = source_uv((0.5, 0.5), fit).expect("the centre is always video");
        assert!((u - 0.5).abs() < 1e-6);
        assert!((v - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_wider_widget_pillarboxes_instead_of_stretching() {
        let fit = fit_factors(1920, 1080, bounds(1240.0, 425.0));
        assert_eq!(fit.1, 1.0, "height is the filled axis");
        assert!(fit.0 < 1.0, "width must be reduced for side bars");

        assert!(source_uv((0.0, 0.5), fit).is_none());
        assert!(source_uv((1.0, 0.5), fit).is_none());

        let margin = (1.0 - fit.0) / 2.0;
        assert!(source_uv((margin, 0.5), fit).is_some());
        assert!(source_uv((1.0 - margin, 0.5), fit).is_some());
        assert!(source_uv((margin - 0.01, 0.5), fit).is_none());
        assert!(source_uv((1.0 - margin + 0.01, 0.5), fit).is_none());
    }

    #[test]
    fn one_frame_fits_differently_at_two_widget_sizes() {
        let wide = fit_factors(1920, 1080, bounds(1280.0, 720.0));
        let ultrawide = fit_factors(1920, 1080, bounds(1920.0, 720.0));
        assert_ne!(wide, ultrawide);
        assert_eq!(wide, (1.0, 1.0));
        assert!(ultrawide.0 < 1.0);
    }

    #[test]
    fn a_taller_widget_letterboxes_top_and_bottom() {
        let fit = fit_factors(1920, 800, bounds(500.0, 500.0));
        assert_eq!(fit.0, 1.0);
        assert!(fit.1 < 1.0);
        assert!(source_uv((0.5, 0.0), fit).is_none());
        assert!(source_uv((0.5, 1.0), fit).is_none());
        assert!(source_uv((0.5, 0.5), fit).is_some());
    }

    #[test]
    fn a_matching_aspect_shows_no_bars_anywhere() {
        let fit = fit_factors(1920, 1080, bounds(1280.0, 720.0));
        assert!((fit.0 - 1.0).abs() < 1e-6 && (fit.1 - 1.0).abs() < 1e-6);
        for target in [(0.0, 0.0), (0.0, 1.0), (1.0, 0.0), (1.0, 1.0), (0.5, 0.5)] {
            assert!(
                source_uv(target, fit).is_some(),
                "a matching aspect must fill the widget: {target:?}"
            );
        }
    }

    #[test]
    fn a_degenerate_fit_draws_nothing_rather_than_dividing_by_zero() {
        assert!(source_uv((0.5, 0.5), (0.0, 1.0)).is_none());
        assert!(source_uv((0.5, 0.5), (1.0, 0.0)).is_none());
    }

    #[test]
    fn a_frame_from_the_player_is_accepted_by_the_surface() {
        let frame = Frame::cpu(2, 2, 8, std::sync::Arc::new(vec![7; 16]), 0, 3);
        assert!(frame.is_well_formed());
        let surface = VideoSurface::new(Some(frame));
        assert!(surface.frame.is_some());
    }

    #[test]
    fn only_a_frame_carrying_a_surface_is_imported() {
        let cpu = Frame::cpu(2, 2, 8, std::sync::Arc::new(vec![7; 16]), 0, 3);
        assert!(!holds_a_gpu_surface(&cpu));
        assert_eq!(surface_key(&cpu), None);
    }
}
