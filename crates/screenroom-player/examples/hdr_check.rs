#[cfg(target_os = "macos")]
fn main() {
    if let Err(error) = receipt::run() {
        eprintln!("hdr_check: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("hdr_check is a macOS-only receipt");
    std::process::exit(1);
}

#[cfg(target_os = "macos")]
mod receipt {
    use std::collections::HashSet;
    use std::time::{Duration, Instant};

    use screenroom_player::gl_render::{
        UPDATE_FRAME, enable_extended_range, target_internal_format,
    };
    use screenroom_player::half_to_f32;
    use screenroom_player::surface::format_fourcc;
    use screenroom_player::{
        Frame, GlRenderer, HdrMode, HwdecMode, LibMpv, Mpv, OutputDescription, OutputTransfer,
        PixelFormat, Player, ToneMapping,
    };

    const SDR_WHITE_NITS: f64 = 100.0;
    const M1: f64 = 0.159_301_757_812_5;
    const M2: f64 = 78.843_75;
    const C1: f64 = 0.835_937_5;
    const C2: f64 = 18.851_562_5;
    const C3: f64 = 18.687_5;

    #[derive(Default)]
    struct Stats {
        samples: usize,
        mean_luma: f64,
        peak_luma: f64,
        mean_green_code: f64,
        peak_green_code: f64,
        mean_luma_code: f64,
        peak_luma_code: f64,
        distinct_green: usize,
        off_grid: usize,
        above_sdr_white: usize,
        above_one: usize,
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Target {
        Sdr8,
        Float8,
        Extended,
    }

    impl Target {
        fn label(self) -> &'static str {
            match self {
                Self::Sdr8 => "sdr",
                Self::Float8 => "float8",
                Self::Extended => "extended",
            }
        }
    }

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let mut arguments = std::env::args().skip(1);
        let file = arguments
            .next()
            .ok_or("usage: hdr_check <file> <seconds> [both|sdr|extended]")?;
        let seconds: f64 = arguments
            .next()
            .ok_or("usage: hdr_check <file> <seconds> [both|sdr|extended]")?
            .parse()?;
        if seconds <= 0.0 {
            return Err("seconds must be greater than zero".into());
        }
        let mode = arguments.next().unwrap_or_else(|| "both".to_string());
        let peak_override: Option<u32> = arguments.next().and_then(|value| value.parse().ok());
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
        println!("source {file}");
        println!("libmpv {} via {}", library.api_version(), library.path());

        if mode == "player" {
            return player_pass(&file, HdrMode::Extended);
        }
        if mode == "player-sdr" {
            return player_pass(&file, HdrMode::Sdr);
        }
        let wanted: &[Target] = match mode.as_str() {
            "both" => &[Target::Sdr8, Target::Extended],
            "all" => &[Target::Sdr8, Target::Float8, Target::Extended],
            "sdr" => &[Target::Sdr8],
            "float8" => &[Target::Float8],
            "extended" => &[Target::Extended],
            other => return Err(format!("unknown mode {other}").into()),
        };
        let mut failures = Vec::new();
        let mut measured: Vec<(Target, Pass)> = Vec::new();
        for target in wanted {
            let pass = pass(&library, &file, seconds, *target, peak_override)?;
            report(target.label(), &pass);
            measured.push((*target, pass));
        }
        let of = |target: Target| {
            measured
                .iter()
                .find(|(wanted, _)| *wanted == target)
                .map(|(_, pass)| pass)
        };
        if let Some(sdr) = of(Target::Sdr8) {
            if sdr.stats.off_grid != 0 {
                failures.push("the eight-bit surface carried values off the eight-bit grid");
            }
            if sdr.stats.samples == 0 || sdr.stats.peak_green_code <= 0.0 {
                failures.push("the eight-bit path delivered a black picture");
            }
        }
        if let Some(float8) = of(Target::Float8) {
            if float8.output.extended_range_requested {
                failures.push("the float control asked libmpv for extended range");
            }
            if !float8.output.format.is_extended_range() {
                failures.push("the float control did not create a half-float surface");
            }
        }
        if let Some(extended) = of(Target::Extended) {
            if extended.output.format != PixelFormat::Rgba16Float {
                failures.push("the extended pass did not create a half-float surface");
            }
            if extended.stats.off_grid == 0 {
                failures.push("the half-float surface carried only eight-bit code values");
            }
            if extended.stats.distinct_green <= 256 {
                failures.push("the half-float surface did not exceed 256 distinct values");
            }
            if extended.stats.above_sdr_white == 0 {
                failures.push("no sampled pixel rose above SDR white");
            }
            if mode == "both" || mode == "all" {
                print_comparison(extended);
            }
        }
        if let (Some(sdr), Some(float8), Some(extended)) =
            (of(Target::Sdr8), of(Target::Float8), of(Target::Extended))
        {
            println!();
            println!(
                "control_float8_vs_sdr_mean_green {:.6} vs {:.6}",
                float8.stats.mean_green_code, sdr.stats.mean_green_code
            );
            println!(
                "control_float8_off_grid {} of {}",
                float8.stats.off_grid, float8.stats.samples
            );
            println!(
                "control_extended_mean_green {:.6}",
                extended.stats.mean_green_code
            );
        }
        if !failures.is_empty() {
            return Err(failures.join("; ").into());
        }
        Ok(())
    }

    fn player_pass(file: &str, hdr: HdrMode) -> Result<(), Box<dyn std::error::Error>> {
        let player = Player::start_with_hdr(HwdecMode::AutoCopy, hdr)?;
        player.load(file);
        let began = Instant::now();
        loop {
            let snapshot = player.snapshot();
            if let Some(failure) = &snapshot.failure {
                return Err(format!("the player failed: {}", failure.message).into());
            }
            if let Some(frame) = &snapshot.frame {
                println!();
                println!("[player] output {}", snapshot.output.label());
                println!(
                    "[player] extended_range_requested {}",
                    snapshot.output.extended_range_requested
                );
                println!(
                    "[player] frame_format {}",
                    match frame.format {
                        PixelFormat::Rgba16Float => "RGBA16Float",
                        PixelFormat::Bgra8 => "BGRA8",
                        PixelFormat::Rgba8 => "RGBA8",
                    }
                );
                println!(
                    "[player] frame_bytes_per_pixel {}",
                    frame.format.bytes_per_pixel()
                );
                println!("[player] extended {}", frame.format.is_extended_range());
                if snapshot.output.extended_range_requested != frame.format.is_extended_range() {
                    return Err("the published output description disagrees with the frame".into());
                }
                return Ok(());
            }
            if began.elapsed() > Duration::from_secs(15) {
                return Err("the player delivered no frame within fifteen seconds".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    struct Pass {
        output: OutputDescription,
        surface: Option<SurfaceFrameReading>,
        stats: Stats,
    }

    struct SurfaceFrameReading {
        fourcc: &'static str,
        number: u32,
        bits_per_channel: u8,
        bytes_per_row: u32,
        internal_format: u32,
    }

    fn pass(
        library: &std::sync::Arc<LibMpv>,
        file: &str,
        seconds: f64,
        target: Target,
        peak_override: Option<u32>,
    ) -> Result<Pass, Box<dyn std::error::Error>> {
        let core = Mpv::new_with_options(
            library,
            &[
                ("vo", "libmpv"),
                ("idle", "yes"),
                ("config", "no"),
                ("terminal", "no"),
                ("aid", "no"),
                ("ao", "null"),
                ("hwdec", "auto-copy"),
                ("loop-file", "inf"),
            ],
        )?;
        let mut renderer = GlRenderer::new(&core)?;
        core.command(&["loadfile", file])?;
        wait_for_file(&core)?;

        let mut output = OutputDescription::default();
        match target {
            Target::Sdr8 => {}
            Target::Float8 => {
                output = OutputDescription {
                    format: PixelFormat::Rgba16Float,
                    extended_range_requested: false,
                    target_peak_nits: None,
                    transfer: OutputTransfer::Srgb,
                    tone_mapping: ToneMapping::MpvDefault,
                };
                renderer.set_output(output.clone());
            }
            Target::Extended => {
                let peak = peak_override.or_else(|| read_peak_nits(&core));
                output = enable_extended_range(&core, peak)?;
                renderer.set_output(output.clone());
            }
        }

        let began = Instant::now();
        let deadline = began + Duration::from_secs_f64(seconds);
        let mut delivered = 0_u64;
        let mut last: Option<Frame> = None;
        let mut size = (0, 0);
        let mut next_size_probe = began;
        while Instant::now() < deadline {
            if Instant::now() >= next_size_probe {
                next_size_probe = Instant::now() + Duration::from_millis(100);
                if let (Ok(width), Ok(height)) = (core.get_i64("dwidth"), core.get_i64("dheight"))
                    && width > 0
                    && height > 0
                    && size != (width as u32, height as u32)
                {
                    size = (width as u32, height as u32);
                    renderer.set_target(size.0, size.1);
                }
            }
            if size.0 == 0 || renderer.update_flags()? & UPDATE_FRAME == 0 {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
            if let Some(frame) = renderer.render()? {
                delivered += 1;
                last = Some(frame);
            }
        }
        let elapsed = began.elapsed().as_secs_f64();
        let frame = last.ok_or_else(|| {
            format!(
                "no frame was delivered in {elapsed:.2} s for {}",
                target.label()
            )
        })?;
        println!(
            "pass {} delivered_chunk_fps {:.3} frames {}",
            target.label(),
            delivered as f64 / elapsed,
            delivered
        );
        let surface = frame.surface.as_ref().map(|surface| SurfaceFrameReading {
            fourcc: format_fourcc(surface.format()),
            number: surface.pixel_format_number(),
            bits_per_channel: surface.format().bits_per_channel(),
            bytes_per_row: surface.bytes_per_row(),
            internal_format: target_internal_format(surface.format()),
        });
        let stats = measure(&frame, target == Target::Extended);
        Ok(Pass {
            output,
            surface,
            stats,
        })
    }

    fn wait_for_file(core: &Mpv) -> Result<(), Box<dyn std::error::Error>> {
        let mut waited = Duration::ZERO;
        while waited < Duration::from_secs(10) {
            if core.get_flag("eof-reached").unwrap_or(false) {
                return Err("the file reached its end before rendering".into());
            }
            if core.get_i64("dwidth").unwrap_or(0) > 0 {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(20));
            waited += Duration::from_millis(20);
        }
        Err("the file did not report a video size within ten seconds".into())
    }

    fn read_peak_nits(core: &Mpv) -> Option<u32> {
        let max_cll = core.get_f64("video-params/max-cll").unwrap_or(0.0);
        let max_luma = core.get_f64("video-params/max-luma").unwrap_or(0.0);
        let peak = max_cll.max(max_luma);
        (peak.is_finite() && peak >= 1.0).then(|| peak.round().clamp(1.0, 10_000.0) as u32)
    }

    fn report(label: &str, pass: &Pass) {
        println!();
        println!("[{label}] output {}", pass.output.label());
        println!(
            "[{label}] bits_per_channel {}",
            pass.output.bits_per_channel()
        );
        println!(
            "[{label}] extended_range_requested {}",
            pass.output.extended_range_requested
        );
        println!("[{label}] transfer {}", pass.output.transfer.as_str());
        println!(
            "[{label}] tone_mapping {}",
            pass.output.tone_mapping.as_str()
        );
        if let Some(surface) = &pass.surface {
            println!(
                "[{label}] surface_fourcc {} number {:#010x}",
                surface.fourcc, surface.number
            );
            println!(
                "[{label}] surface_bits_per_channel {}",
                surface.bits_per_channel
            );
            println!("[{label}] surface_bytes_per_row {}", surface.bytes_per_row);
            println!(
                "[{label}] fbo_internal_format {:#06x}",
                surface.internal_format
            );
        }
        println!("[{label}] samples {}", pass.stats.samples);
        println!(
            "[{label}] mean_green_code {:.6}",
            pass.stats.mean_green_code
        );
        println!(
            "[{label}] peak_green_code {:.6}",
            pass.stats.peak_green_code
        );
        println!("[{label}] distinct_green {}", pass.stats.distinct_green);
        println!("[{label}] off_grid {}", pass.stats.off_grid);
        println!("[{label}] above_one {}", pass.stats.above_one);
        if pass.output.transfer == OutputTransfer::Pq {
            println!("[{label}] mean_luma_code {:.6}", pass.stats.mean_luma_code);
            println!("[{label}] peak_luma_code {:.6}", pass.stats.peak_luma_code);
            println!("[{label}] mean_nits {:.1}", pass.stats.mean_luma);
            println!("[{label}] peak_nits {:.1}", pass.stats.peak_luma);
            println!(
                "[{label}] peak_vs_sdr_white {:.3}",
                pass.stats.peak_luma / SDR_WHITE_NITS
            );
            println!(
                "[{label}] above_sdr_white {} of {}",
                pass.stats.above_sdr_white, pass.stats.samples
            );
        }
    }

    fn print_comparison(extended: &Pass) {
        println!();
        println!(
            "objective_precision {} (distinct green values on the half-float surface against 256 steps of eight bits)",
            extended.stats.distinct_green
        );
        println!("objective_peak_nits {:.1}", extended.stats.peak_luma);
        println!(
            "objective_peak_over_sdr_white {:.3}",
            extended.stats.peak_luma / SDR_WHITE_NITS
        );
        println!("objective_mean_nits {:.1}", extended.stats.mean_luma);
        println!(
            "objective_mean_luma_code {:.6}",
            extended.stats.mean_luma_code
        );
        println!(
            "objective_peak_luma_code {:.6}",
            extended.stats.peak_luma_code
        );
    }

    fn measure(frame: &Frame, pq_encoded: bool) -> Stats {
        let float_surface = frame.format == PixelFormat::Rgba16Float;
        let bytes_per_pixel = frame.format.bytes_per_pixel() as usize;
        let (width, height) = (frame.width as usize, frame.height as usize);
        frame.with_pixels(|bytes, stride| {
            let mut stats = Stats::default();
            let mut green = HashSet::new();
            let mut sum = 0.0_f64;
            let mut sum_luma_code = 0.0_f64;
            let mut sum_green = 0.0_f64;
            let mut y = 0;
            while y < height {
                let mut x = 0;
                while x < width {
                    let offset = y * stride + x * bytes_per_pixel;
                    if let Some(pixel) = bytes.get(offset..offset + bytes_per_pixel) {
                        let (luma, green_value) = if float_surface {
                            let red =
                                pq_to_nits(half_to_f32(u16::from_le_bytes([pixel[0], pixel[1]])));
                            let green =
                                f64::from(half_to_f32(u16::from_le_bytes([pixel[2], pixel[3]])));
                            let blue =
                                pq_to_nits(half_to_f32(u16::from_le_bytes([pixel[4], pixel[5]])));
                            let luma = if pq_encoded {
                                0.2627 * red + 0.678 * pq_to_nits(green as f32) + 0.0593 * blue
                            } else {
                                0.0
                            };
                            (luma, green)
                        } else {
                            (0.0, f64::from(pixel[1]) / 255.0)
                        };
                        stats.samples += 1;
                        sum_green += green_value;
                        stats.peak_green_code = stats.peak_green_code.max(green_value);
                        if pq_encoded {
                            let luma_code = pq_from_nits(luma);
                            sum += luma;
                            sum_luma_code += luma_code;
                            stats.peak_luma = stats.peak_luma.max(luma);
                            stats.peak_luma_code = stats.peak_luma_code.max(luma_code);
                            if luma > SDR_WHITE_NITS {
                                stats.above_sdr_white += 1;
                            }
                        }
                        green.insert(green_value.to_bits());
                        if !is_eight_bit_grid(green_value) {
                            stats.off_grid += 1;
                        }
                        if green_value > 1.0 {
                            stats.above_one += 1;
                        }
                    }
                    x += 7;
                }
                y += 7;
            }
            stats.distinct_green = green.len();
            if stats.samples > 0 {
                stats.mean_green_code = sum_green / stats.samples as f64;
                stats.mean_luma = sum / stats.samples as f64;
                stats.mean_luma_code = sum_luma_code / stats.samples as f64;
            }
            stats
        })
    }

    fn is_eight_bit_grid(value: f64) -> bool {
        let scaled = value * 255.0;
        (scaled - scaled.round()).abs() < 0.000_5
    }

    fn pq_from_nits(nits: f64) -> f64 {
        let ratio = (nits / 10_000.0).clamp(0.0, 1.0).powf(M1);
        ((C1 + C2 * ratio) / (1.0 + C3 * ratio)).powf(M2)
    }

    fn pq_to_nits(encoded: f32) -> f64 {
        let value = f64::from(encoded).clamp(0.0, 1.0);
        let power = value.powf(1.0 / M2);
        let numerator = (power - C1).max(0.0);
        let denominator = C2 - C3 * power;
        if denominator <= 0.0 {
            return 0.0;
        }
        10_000.0 * (numerator / denominator).powf(1.0 / M1)
    }
}
