#[cfg(target_os = "macos")]
fn main() {
    if let Err(error) = harness::run() {
        eprintln!("tear_harness: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("tear_harness is a macOS-only receipt");
    std::process::exit(1);
}

#[cfg(target_os = "macos")]
mod harness {
    use std::collections::{BTreeSet, HashMap};
    use std::time::{Duration, Instant};

    use screenroom_desktop::video_surface;
    use screenroom_player::gl_render::{UPDATE_FRAME, publication_stats};
    use screenroom_player::surface::tearing::{self, Strips, TearWatch, Verdict};
    use screenroom_player::{Frame, GlRenderer, LibMpv, Mpv};

    const READBACK_WIDTH: u32 = 64;
    const READBACK_HEIGHT: u32 = 24;
    const STRIP_ROWS: [u32; 3] = [3, 12, 21];
    const MAX_IMPORTED: usize = 12;

    const READBACK_SHADER: &str = r#"
@group(0) @binding(0) var frame_texture: texture_2d<f32>;
@group(0) @binding(1) var frame_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
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
    return textureSample(frame_texture, frame_sampler, input.uv);
}
"#;

    struct Sample {
        device: wgpu::Device,
        queue: wgpu::Queue,
        layout: wgpu::BindGroupLayout,
        pipeline: wgpu::RenderPipeline,
        sampler: wgpu::Sampler,
        target: wgpu::Texture,
        target_view: wgpu::TextureView,
        readback: wgpu::Buffer,
        imported: HashMap<usize, (wgpu::Texture, wgpu::BindGroup)>,
        imported_size: (u32, u32),
        imports: u64,
        import_failures: u64,
    }

    impl Sample {
        fn new() -> Result<Self, Box<dyn std::error::Error>> {
            use iced::futures::executor::block_on;
            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
            let adapter =
                block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                    .map_err(|error| format!("no wgpu adapter: {error}"))?;
            let (device, queue) =
                block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                    .map_err(|error| format!("no wgpu device: {error}"))?;

            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("tear-harness"),
                source: wgpu::ShaderSource::Wgsl(READBACK_SHADER.into()),
            });
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("tear-harness"),
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
                ],
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("tear-harness"),
                bind_group_layouts: &[&layout],
                push_constant_ranges: &[],
            });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("tear-harness"),
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
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });
            let target = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("tear-harness"),
                size: wgpu::Extent3d {
                    width: READBACK_WIDTH,
                    height: READBACK_HEIGHT,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("tear-harness"),
                size: u64::from(READBACK_WIDTH * READBACK_HEIGHT * 4),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            });
            Ok(Self {
                device,
                queue,
                layout,
                pipeline,
                sampler,
                target,
                target_view,
                readback,
                imported: HashMap::new(),
                imported_size: (0, 0),
                imports: 0,
                import_failures: 0,
            })
        }

        fn strips(&mut self, frame: &Frame) -> Option<Strips> {
            let surface = frame.surface.clone()?;
            if self.imported_size != (frame.width, frame.height)
                || self.imported.len() >= MAX_IMPORTED
            {
                self.imported.clear();
                self.imported_size = (frame.width, frame.height);
            }
            let key = surface.as_ptr() as usize;
            if key == 0 {
                return None;
            }
            if !self.imported.contains_key(&key) {
                let texture = video_surface::surface_texture(&self.device, &surface, frame);
                let Some(texture) = texture else {
                    self.import_failures += 1;
                    return None;
                };
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("tear-harness"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                    ],
                });
                self.imported.insert(key, (texture, bind_group));
                self.imports += 1;
            }
            let bind_group = &self.imported.get(&key)?.1;

            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("tear-harness"),
                });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("tear-harness"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.target_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.target,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &self.readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(READBACK_WIDTH * 4),
                        rows_per_image: Some(READBACK_HEIGHT),
                    },
                },
                wgpu::Extent3d {
                    width: READBACK_WIDTH,
                    height: READBACK_HEIGHT,
                    depth_or_array_layers: 1,
                },
            );
            self.queue.submit(Some(encoder.finish()));

            let slice = self.readback.slice(..);
            slice.map_async(wgpu::MapMode::Read, |_| {});
            let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
            let data = slice.get_mapped_range();
            let stride = (READBACK_WIDTH * 4) as usize;
            let mut rows = [[[0_u8; 4]; 3]; 1];
            rows[0][0] = strip(&data, stride, STRIP_ROWS[0]);
            rows[0][1] = strip(&data, stride, STRIP_ROWS[1]);
            rows[0][2] = strip(&data, stride, STRIP_ROWS[2]);
            let uniform = [
                uniform_strip(&data, stride, STRIP_ROWS[0]),
                uniform_strip(&data, stride, STRIP_ROWS[1]),
                uniform_strip(&data, stride, STRIP_ROWS[2]),
            ];
            drop(data);
            self.readback.unmap();
            Some(Strips {
                top: tearing::Strip {
                    color: rows[0][0],
                    uniform: uniform[0],
                },
                middle: tearing::Strip {
                    color: rows[0][1],
                    uniform: uniform[1],
                },
                bottom: tearing::Strip {
                    color: rows[0][2],
                    uniform: uniform[2],
                },
            })
        }
    }

    fn strip(data: &[u8], stride: usize, row: u32) -> [u8; 4] {
        let offset = row as usize * stride + (READBACK_WIDTH as usize / 2) * 4;
        match data.get(offset..offset + 4) {
            Some(pixel) => [pixel[0], pixel[1], pixel[2], pixel[3]],
            None => [0, 0, 0, 0],
        }
    }

    fn uniform_strip(data: &[u8], stride: usize, row: u32) -> bool {
        let reference = strip(data, stride, row);
        (0..READBACK_WIDTH).all(|column| {
            let offset = row as usize * stride + column as usize * 4;
            match data.get(offset..offset + 4) {
                Some(pixel) => tearing::near(reference, [pixel[0], pixel[1], pixel[2], pixel[3]]),
                None => false,
            }
        })
    }

    pub struct Report {
        published: u64,
        consumed: u64,
        unique_generations: usize,
        imports: u64,
        import_failures: u64,
        samples: u64,
        torn: u64,
        stale: u64,
        unjudged: u64,
        fps: f64,
        render_ms_mean: f64,
        render_ms_worst: f64,
        pending_max: usize,
        fence_timeouts: u64,
        skipped: u64,
        tail_waits: u64,
        publish_wait_max_ms: u64,
        failure: Option<String>,
    }

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let mut arguments = std::env::args().skip(1);
        let file = arguments
            .next()
            .ok_or("usage: tear_harness <file> <seconds> [report.csv]")?;
        let seconds: f64 = arguments
            .next()
            .ok_or("usage: tear_harness <file> <seconds> [report.csv]")?
            .parse()?;
        if !(seconds.is_finite() && seconds > 0.0) {
            return Err("seconds must be greater than zero".into());
        }
        let report_path = arguments.next();
        let path =
            std::fs::canonicalize(&file).map_err(|error| format!("cannot open {file}: {error}"))?;
        if !path.is_file() {
            return Err("media path must be a file".into());
        }
        let file = path
            .to_str()
            .ok_or("the media path is not valid UTF-8")?
            .to_string();

        let library = LibMpv::load()?;
        let core = Mpv::new_with_options(
            &library,
            &[
                ("vo", "libmpv"),
                ("idle", "yes"),
                ("config", "no"),
                ("terminal", "no"),
                ("aid", "no"),
                ("ao", "null"),
                ("hwdec", "auto"),
                ("loop-file", "inf"),
            ],
        )?;
        let mut renderer = GlRenderer::new(&core)?;
        let mut sample = Sample::new()?;
        let mut watch = TearWatch::default();
        let mut seen = BTreeSet::new();
        let mut published = 0_u64;
        let mut unjudged = 0_u64;
        let mut torn = 0_u64;
        let mut stale = 0_u64;
        let mut imports = 0_u64;
        let mut import_failures = 0_u64;
        let mut render_total = Duration::ZERO;
        let mut render_worst = Duration::ZERO;
        let mut pending_max = 0_usize;
        let mut failure: Option<String> = None;

        core.command(&["loadfile", &file])?;

        let began = Instant::now();
        let deadline = began + Duration::from_secs_f64(seconds);
        let mut target = (0, 0);
        let mut next_size_probe = began;

        while Instant::now() < deadline {
            if Instant::now() >= next_size_probe {
                next_size_probe = Instant::now() + Duration::from_millis(250);
                if let Ok(width) = core.get_i64("dwidth")
                    && let Ok(height) = core.get_i64("dheight")
                    && width > 0
                    && height > 0
                    && target != (width as u32, height as u32)
                {
                    target = (width as u32, height as u32);
                    renderer.set_target(target.0, target.1);
                }
            }
            if target.0 == 0 {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }
            let started = Instant::now();
            let frame = if renderer.update_flags()? & UPDATE_FRAME != 0 {
                renderer.render()?
            } else if renderer.has_pending() {
                renderer.publish_pending()?
            } else {
                None
            };
            let took = started.elapsed();
            if let Some(frame) = frame {
                render_total += took;
                render_worst = render_worst.max(took);
                published += 1;
                pending_max = pending_max.max(renderer.pending_len());
                if !seen.insert(frame.generation) {
                    failure = Some(format!(
                        "generation {} was published twice",
                        frame.generation
                    ));
                    break;
                }
                if let Some(surface) = frame.surface.as_ref() {
                    surface.reads().note_reading(frame.generation);
                }
                imports = sample.imports;
                import_failures = sample.import_failures;
                let strips = sample.strips(&frame);
                if let Some(surface) = frame.surface.as_ref() {
                    surface.reads().note_released(frame.generation);
                }
                match strips {
                    Some(strips) => match watch.observe(strips) {
                        Verdict::Torn => torn += 1,
                        Verdict::Stale => stale += 1,
                        Verdict::Unjudgeable => unjudged += 1,
                        Verdict::Clean => {}
                    },
                    None => unjudged += 1,
                }
            } else {
                std::thread::sleep(Duration::from_millis(2));
            }
        }

        while renderer.has_pending() && failure.is_none() {
            let Some(frame) = renderer.publish_pending()? else {
                break;
            };
            published += 1;
            if !seen.insert(frame.generation) {
                failure = Some(format!(
                    "generation {} was published twice",
                    frame.generation
                ));
                break;
            }
            if let Some(surface) = frame.surface.as_ref() {
                surface.reads().note_reading(frame.generation);
            }
            if let Some(strips) = sample.strips(&frame) {
                match watch.observe(strips) {
                    Verdict::Torn => torn += 1,
                    Verdict::Stale => stale += 1,
                    Verdict::Unjudgeable => unjudged += 1,
                    Verdict::Clean => {}
                }
            } else {
                unjudged += 1;
            }
            if let Some(surface) = frame.surface.as_ref() {
                surface.reads().note_released(frame.generation);
            }
        }

        let stats = publication_stats();
        let elapsed = began.elapsed().as_secs_f64().max(0.001);
        let consumed = renderer.generations_consumed();
        if failure.is_none() && import_failures > 0 {
            failure = Some(format!("{import_failures} surfaces could not be imported"));
        }
        if failure.is_none() && published == 0 {
            failure = Some("no frame was published".to_string());
        }
        if failure.is_none() && stats.generations_published != published {
            failure = Some(format!(
                "the renderer published {} frames but the harness saw {published}",
                stats.generations_published
            ));
        }
        if failure.is_none() && consumed != published {
            failure = Some(format!(
                "{published} frames were published but {consumed} were consumed"
            ));
        }
        if failure.is_none() && torn > 0 {
            failure = Some(format!("{torn} torn frames were sampled"));
        }
        if failure.is_none() && stale > 0 {
            failure = Some(format!("{stale} frames repeated the previous picture"));
        }

        let report = Report {
            published,
            consumed,
            unique_generations: seen.len(),
            imports,
            import_failures,
            samples: watch.samples(),
            torn,
            stale,
            unjudged,
            fps: published as f64 / elapsed,
            render_ms_mean: render_total.as_secs_f64() * 1_000.0 / published.max(1) as f64,
            render_ms_worst: render_worst.as_secs_f64() * 1_000.0,
            pending_max,
            fence_timeouts: stats.fence_timeouts,
            skipped: stats.skipped,
            tail_waits: stats.tail_waits,
            publish_wait_max_ms: stats.publish_wait_max_ms,
            failure: failure.clone(),
        };

        println!("media {file}");
        println!("libmpv {} via {}", library.api_version(), library.path());
        println!("target {}x{}", target.0, target.1);
        println!("sampled_seconds {:.3}", elapsed);
        println!("delivered_fps {:.3}", report.fps);
        println!("published {}", report.published);
        println!("consumed {}", report.consumed);
        println!("unique_generations {}", report.unique_generations);
        println!("imports {}", report.imports);
        println!("import_failures {}", report.import_failures);
        println!("render_ms_mean {:.3}", report.render_ms_mean);
        println!("render_ms_worst {:.3}", report.render_ms_worst);
        println!("pending_max {}", report.pending_max);
        println!("fence_timeouts {}", report.fence_timeouts);
        println!("skipped {}", report.skipped);
        println!("tail_waits {}", report.tail_waits);
        println!("publish_wait_max_ms {}", report.publish_wait_max_ms);
        println!("tear_samples {}", report.samples);
        println!("tear_mismatches {}", report.torn);
        println!("stale_repeats {}", report.stale);
        println!("unjudged_frames {}", report.unjudged);
        println!(
            "failure {}",
            report.failure.clone().unwrap_or_else(|| "none".to_string())
        );

        if let Some(path) = report_path {
            let rows = [
                ("media", file.clone()),
                ("sampled_seconds", format!("{elapsed:.3}")),
                ("delivered_fps", format!("{:.3}", report.fps)),
                ("published", report.published.to_string()),
                ("consumed", report.consumed.to_string()),
                ("unique_generations", report.unique_generations.to_string()),
                ("imports", report.imports.to_string()),
                ("import_failures", report.import_failures.to_string()),
                ("render_ms_mean", format!("{:.3}", report.render_ms_mean)),
                ("render_ms_worst", format!("{:.3}", report.render_ms_worst)),
                ("pending_max", report.pending_max.to_string()),
                ("fence_timeouts", report.fence_timeouts.to_string()),
                ("skipped", report.skipped.to_string()),
                ("tail_waits", report.tail_waits.to_string()),
                (
                    "publish_wait_max_ms",
                    report.publish_wait_max_ms.to_string(),
                ),
                ("tear_samples", report.samples.to_string()),
                ("tear_mismatches", report.torn.to_string()),
                ("stale_repeats", report.stale.to_string()),
                ("unjudged_frames", report.unjudged.to_string()),
                (
                    "failure",
                    report.failure.clone().unwrap_or_else(|| "none".to_string()),
                ),
            ];
            let mut csv = String::from("key,value\n");
            for (key, value) in rows {
                if value.contains([',', '"', '\n']) {
                    csv.push_str(&format!("\"{}\",\"{}\"\n", key, value.replace('"', "\"\"")));
                } else {
                    csv.push_str(&format!("{key},{value}\n"));
                }
            }
            std::fs::write(&path, csv)
                .map_err(|error| format!("could not write {path}: {error}"))?;
            println!("report {path}");
        }

        match failure {
            Some(reason) => Err(reason.into()),
            None => Ok(()),
        }
    }
}
