#[cfg(target_os = "macos")]
fn main() {
    if let Err(error) = receipt::run() {
        eprintln!("gl_render_check: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("gl_render_check is a macOS-only receipt");
    std::process::exit(1);
}

#[cfg(target_os = "macos")]
mod receipt {
    use std::time::{Duration, Instant};

    use screenroom_player::gl_render::UPDATE_FRAME;
    use screenroom_player::{Frame, GlRenderer, LibMpv, Mpv};

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let mut arguments = std::env::args().skip(1);
        let file = arguments
            .next()
            .ok_or("usage: gl_render_check <file> <seconds>")?;
        let seconds: f64 = arguments
            .next()
            .ok_or("usage: gl_render_check <file> <seconds>")?
            .parse()?;
        if seconds <= 0.0 {
            return Err("seconds must be greater than zero".into());
        }
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
                ("hwdec", "auto-copy"),
                ("loop-file", "inf"),
            ],
        )?;
        let mut renderer = GlRenderer::new(&core)?;
        core.command(&["loadfile", &file])?;

        let began = Instant::now();
        let deadline = began + Duration::from_secs_f64(seconds);
        let mut delivered = 0_u64;
        let mut total = Duration::ZERO;
        let mut worst = Duration::ZERO;
        let mut last: Option<Frame> = None;
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
            if target.0 == 0 || renderer.update_flags()? & UPDATE_FRAME == 0 {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }
            let started = Instant::now();
            if let Some(frame) = renderer.render()? {
                let took = started.elapsed();
                delivered += 1;
                total += took;
                worst = worst.max(took);
                last = Some(frame);
            }
        }

        let elapsed = began.elapsed().as_secs_f64();
        println!("source {file}");
        println!("libmpv {} via {}", library.api_version(), library.path());

        let Some(frame) = last else {
            println!("delivered_fps 0.000");
            println!("render_ms_mean 0.000");
            println!("render_ms_worst 0.000");
            return Err(format!("no frame was delivered in {elapsed:.2} s").into());
        };

        println!(
            "frame {}x{} stride {}",
            frame.width, frame.height, frame.stride
        );
        println!(
            "delivered_fps {:.3} ({delivered} frames in {elapsed:.2} s)",
            delivered as f64 / elapsed
        );
        println!(
            "render_ms_mean {:.3}",
            total.as_secs_f64() * 1_000.0 / delivered as f64
        );
        println!("render_ms_worst {:.3}", worst.as_secs_f64() * 1_000.0);
        println!("skipped_frames {}", renderer.skipped());

        let centre = frame
            .pixel(frame.width / 2, frame.height / 2)
            .ok_or("the centre of the surface could not be read")?;
        println!(
            "centre BGRA {},{},{},{}",
            centre[0], centre[1], centre[2], centre[3]
        );
        let (non_black, sampled) = sample(&frame);
        println!(
            "non_black_pct {:.2}",
            100.0 * non_black as f64 / sampled.max(1) as f64
        );
        println!("sampled_pixels {sampled}");

        if non_black == 0 {
            return Err("the published surface is blank".into());
        }
        Ok(())
    }

    fn sample(frame: &Frame) -> (usize, usize) {
        frame.with_pixels(|bytes, stride| {
            let mut non_black = 0;
            let mut sampled = 0;
            let mut y = 100;
            while y < frame.height as usize {
                let mut x = 100;
                while x < frame.width as usize {
                    let offset = y * stride + x * 4;
                    if let Some(pixel) = bytes.get(offset..offset + 4) {
                        sampled += 1;
                        if pixel[0] != 0 || pixel[1] != 0 || pixel[2] != 0 {
                            non_black += 1;
                        }
                    }
                    x += 97;
                }
                y += 97;
            }
            (non_black, sampled)
        })
    }
}
