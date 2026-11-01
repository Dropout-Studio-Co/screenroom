use std::sync::Arc;

#[cfg(target_os = "macos")]
use crate::SurfaceFrame;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PixelFormat {
    Rgba8,

    Bgra8,

    Rgba16Float,
}

impl PixelFormat {
    pub fn bytes_per_pixel(self) -> u32 {
        match self {
            Self::Rgba8 | Self::Bgra8 => 4,
            Self::Rgba16Float => 8,
        }
    }

    pub fn bits_per_channel(self) -> u8 {
        match self {
            Self::Rgba8 | Self::Bgra8 => 8,
            Self::Rgba16Float => 16,
        }
    }

    pub fn is_extended_range(self) -> bool {
        matches!(self, Self::Rgba16Float)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputTransfer {
    Srgb,

    Pq,
}

impl OutputTransfer {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Srgb => "srgb",
            Self::Pq => "pq",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToneMapping {
    MpvDefault,

    Passthrough,
}

impl ToneMapping {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MpvDefault => "mpv default",
            Self::Passthrough => "none",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputDescription {
    pub format: PixelFormat,

    pub extended_range_requested: bool,

    pub target_peak_nits: Option<u32>,

    pub transfer: OutputTransfer,

    pub tone_mapping: ToneMapping,
}

impl OutputDescription {
    pub fn sdr_bgra8() -> Self {
        Self {
            format: PixelFormat::Bgra8,
            extended_range_requested: false,
            target_peak_nits: None,
            transfer: OutputTransfer::Srgb,
            tone_mapping: ToneMapping::MpvDefault,
        }
    }

    pub fn extended_float(target_peak_nits: Option<u32>) -> Self {
        Self {
            format: PixelFormat::Rgba16Float,
            extended_range_requested: true,
            target_peak_nits,
            transfer: OutputTransfer::Pq,
            tone_mapping: ToneMapping::Passthrough,
        }
    }

    pub fn bits_per_channel(&self) -> u8 {
        self.format.bits_per_channel()
    }

    pub fn is_extended_range(&self) -> bool {
        self.format.is_extended_range() && self.extended_range_requested
    }

    pub fn label(&self) -> String {
        if !self.format.is_extended_range() {
            return format!(
                "{} bits per channel, {}, tone mapping {}",
                self.bits_per_channel(),
                self.transfer.as_str(),
                self.tone_mapping.as_str()
            );
        }
        if !self.extended_range_requested {
            return format!(
                "{} bits per channel, {}, extended range not requested",
                self.bits_per_channel(),
                self.transfer.as_str()
            );
        }
        let peak = match self.target_peak_nits {
            Some(nits) => format!(", target peak {nits} nits"),
            None => String::new(),
        };
        format!(
            "{} bits per channel, {}/bt.2020, tone mapping {}{peak}",
            self.bits_per_channel(),
            self.transfer.as_str(),
            self.tone_mapping.as_str()
        )
    }
}

impl Default for OutputDescription {
    fn default() -> Self {
        Self::sdr_bgra8()
    }
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub width: u32,

    pub height: u32,

    pub stride: u32,

    pub rgba: Arc<Vec<u8>>,

    pub offset: usize,

    pub generation: u64,

    pub format: PixelFormat,

    #[cfg(target_os = "macos")]
    pub surface: Option<Arc<SurfaceFrame>>,
}

impl Frame {
    pub fn cpu(
        width: u32,
        height: u32,
        stride: u32,
        rgba: Arc<Vec<u8>>,
        offset: usize,
        generation: u64,
    ) -> Self {
        Self {
            width,
            height,
            stride,
            rgba,
            offset,
            generation,
            format: PixelFormat::Rgba8,
            #[cfg(target_os = "macos")]
            surface: None,
        }
    }

    #[cfg(target_os = "macos")]
    pub fn gpu(width: u32, height: u32, surface: Arc<SurfaceFrame>, generation: u64) -> Self {
        Self {
            width,
            height,
            stride: surface.bytes_per_row(),
            rgba: Arc::new(Vec::new()),
            offset: 0,
            generation,
            format: surface.format(),
            surface: Some(surface),
        }
    }

    pub fn is_bgra(&self) -> bool {
        self.format == PixelFormat::Bgra8
    }

    pub fn with_pixels<R>(&self, f: impl FnOnce(&[u8], usize) -> R) -> R {
        #[cfg(target_os = "macos")]
        if let Some(surface) = &self.surface {
            return surface.with_pixels(|bytes| f(bytes, surface.bytes_per_row() as usize));
        }
        let start = self.offset.min(self.rgba.len());
        f(&self.rgba[start..], self.stride as usize)
    }

    pub fn is_well_formed(&self) -> bool {
        if self.width == 0 || self.height == 0 {
            return false;
        }
        let row_bytes = self.width as usize * self.format.bytes_per_pixel() as usize;
        #[cfg(target_os = "macos")]
        if let Some(surface) = &self.surface {
            return surface.width() == self.width
                && surface.height() == self.height
                && surface.format() == self.format
                && surface.bytes_per_row() as usize >= row_bytes;
        }
        self.stride as usize >= row_bytes
            && self.rgba.len() >= self.offset + self.stride as usize * self.height as usize
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if self.format.is_extended_range() {
            return self.extended_pixel(x, y).map(|pixel| {
                [
                    to_unorm8(pixel[0]),
                    to_unorm8(pixel[1]),
                    to_unorm8(pixel[2]),
                    to_unorm8(pixel[3]),
                ]
            });
        }
        if x >= self.width || y >= self.height || !self.is_well_formed() {
            return None;
        }
        #[cfg(target_os = "macos")]
        if let Some(surface) = &self.surface {
            return surface
                .read_pixel(x, y)
                .map(|pixel| to_rgba(pixel, self.is_bgra()));
        }
        let bgra = self.is_bgra();
        self.with_pixels(|bytes, stride| {
            let offset = y as usize * stride + x as usize * 4;
            Some(to_rgba(
                [
                    bytes[offset],
                    bytes[offset + 1],
                    bytes[offset + 2],
                    bytes[offset + 3],
                ],
                bgra,
            ))
        })
    }

    pub fn extended_pixel(&self, x: u32, y: u32) -> Option<[f32; 4]> {
        if !self.format.is_extended_range()
            || x >= self.width
            || y >= self.height
            || !self.is_well_formed()
        {
            return None;
        }
        #[cfg(target_os = "macos")]
        if let Some(surface) = &self.surface {
            return surface.read_extended_pixel(x, y);
        }
        self.with_pixels(|bytes, stride| {
            let offset = y as usize * stride + x as usize * 8;
            let pixel = bytes.get(offset..offset + 8)?;
            Some([
                half_to_f32(u16::from_le_bytes([pixel[0], pixel[1]])),
                half_to_f32(u16::from_le_bytes([pixel[2], pixel[3]])),
                half_to_f32(u16::from_le_bytes([pixel[4], pixel[5]])),
                half_to_f32(u16::from_le_bytes([pixel[6], pixel[7]])),
            ])
        })
    }

    pub fn is_blank(&self) -> bool {
        let row_bytes = self.width as usize * self.format.bytes_per_pixel() as usize;
        self.with_pixels(|bytes, stride| {
            (0..self.height as usize).all(|y| {
                let start = y * stride;
                bytes.get(start..start + row_bytes).is_some_and(|row| {
                    row.as_chunks::<4>()
                        .0
                        .iter()
                        .all(|pixel| pixel == &[0, 0, 0, 0])
                })
            })
        })
    }

    pub fn tight_pixels(&self) -> Vec<u8> {
        let row_bytes = self.width as usize * 4;
        let mut out = vec![0u8; row_bytes * self.height as usize];
        if !self.is_well_formed() {
            return out;
        }
        if self.format.is_extended_range() {
            self.with_pixels(|bytes, stride| {
                for (y, row) in out.chunks_exact_mut(row_bytes).enumerate() {
                    for (x, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                        let offset = y * stride + x * 8;
                        let Some(raw) = bytes.get(offset..offset + 8) else {
                            continue;
                        };
                        *pixel = [
                            to_unorm8(half_to_f32(u16::from_le_bytes([raw[0], raw[1]]))),
                            to_unorm8(half_to_f32(u16::from_le_bytes([raw[2], raw[3]]))),
                            to_unorm8(half_to_f32(u16::from_le_bytes([raw[4], raw[5]]))),
                            to_unorm8(half_to_f32(u16::from_le_bytes([raw[6], raw[7]]))),
                        ];
                    }
                }
            });
            return out;
        }
        self.with_pixels(|bytes, stride| {
            for (y, row) in out.chunks_exact_mut(row_bytes).enumerate() {
                let start = y * stride;
                row.copy_from_slice(&bytes[start..start + row_bytes]);
            }
        });
        if self.is_bgra() {
            swizzle_bgra_to_rgba(&mut out);
        }
        out
    }
}

fn to_rgba(pixel: [u8; 4], bgra: bool) -> [u8; 4] {
    if bgra {
        [pixel[2], pixel[1], pixel[0], pixel[3]]
    } else {
        pixel
    }
}

fn swizzle_bgra_to_rgba(bytes: &mut [u8]) {
    for pixel in bytes.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
}

pub fn half_to_f32(bits: u16) -> f32 {
    let sign = u32::from(bits >> 15);
    let exponent = u32::from((bits >> 10) & 0x1f);
    let mantissa = u32::from(bits & 0x3ff);
    let magnitude = if exponent == 0 {
        mantissa as f32 * 2f32.powi(-24)
    } else if exponent == 31 {
        if mantissa == 0 {
            f32::INFINITY
        } else {
            f32::NAN
        }
    } else {
        (1.0 + mantissa as f32 / 1024.0) * 2f32.powi(exponent as i32 - 15)
    };
    if sign == 1 { -magnitude } else { magnitude }
}

fn to_unorm8(value: f32) -> u8 {
    if !value.is_finite() {
        return 0;
    }
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: u32, height: u32, bytes: Vec<u8>) -> Frame {
        Frame::cpu(width, height, width * 4, Arc::new(bytes), 0, 1)
    }

    fn padded(width: u32, height: u32, stride: u32) -> Frame {
        let mut bytes = vec![0xAAu8; stride as usize * height as usize];
        for y in 0..height as usize {
            for x in 0..width as usize {
                let offset = y * stride as usize + x * 4;
                bytes[offset..offset + 4].copy_from_slice(&[10, 20, 30, 255]);
            }
        }
        Frame::cpu(width, height, stride, Arc::new(bytes), 0, 2)
    }

    fn half_frame(width: u32, height: u32, values: &[[f32; 4]]) -> Frame {
        let mut bytes = Vec::with_capacity(values.len() * 8);
        for pixel in values {
            for channel in pixel {
                bytes.extend_from_slice(&f32_to_half(*channel).to_le_bytes());
            }
        }
        let mut frame = Frame::cpu(width, height, width * 8, Arc::new(bytes), 0, 3);
        frame.format = PixelFormat::Rgba16Float;
        frame
    }

    fn f32_to_half(value: f32) -> u16 {
        let bits = value.to_bits();
        let sign = ((bits >> 16) & 0x8000) as u16;
        let exponent = ((bits >> 23) & 0xff) as i32;
        let mantissa = bits & 0x7f_ffff;
        if exponent == 0 {
            return sign;
        }
        if exponent == 0xff {
            return sign | 0x7c00 | if mantissa == 0 { 0 } else { 0x200 };
        }
        let unbiased = exponent - 127;
        if unbiased > 15 {
            return sign | 0x7c00;
        }
        if unbiased < -14 {
            let shift = (-14 - unbiased) as u32;
            let value = (mantissa | 0x80_0000) >> (shift + 13);
            return sign | value as u16;
        }
        let half_exponent = ((unbiased + 15) as u16) << 10;
        sign | half_exponent | (mantissa >> 13) as u16
    }

    #[test]
    fn well_formed_checks_buffer_length() {
        assert!(frame(2, 2, vec![0; 16]).is_well_formed());
        assert!(!frame(2, 2, vec![0; 15]).is_well_formed());
        assert!(!frame(0, 2, vec![]).is_well_formed());
        assert!(padded(2, 2, 16).is_well_formed());
        let narrow = Frame::cpu(2, 2, 4, Arc::new(vec![0; 8]), 0, 3);
        assert!(
            !narrow.is_well_formed(),
            "a row narrower than its pixels is invalid"
        );
    }

    #[test]
    fn an_alignment_prefix_is_outside_the_pixels_and_still_valid() {
        let mut bytes = vec![0xEEu8; 64];
        bytes.extend_from_slice(&[10, 20, 30, 255].repeat(4));
        let frame = Frame::cpu(2, 2, 8, Arc::new(bytes), 64, 4);
        assert!(frame.is_well_formed());
        assert_eq!(frame.pixel(0, 0), Some([10, 20, 30, 255]));
        assert_eq!(frame.tight_pixels(), [10, 20, 30, 255].repeat(4));
        assert_eq!(
            frame.with_pixels(|bytes, stride| (bytes.len(), stride)),
            (16, 8)
        );
    }

    #[test]
    fn an_offset_past_the_buffer_is_not_well_formed() {
        let frame = Frame::cpu(2, 2, 8, Arc::new(vec![0; 64]), 60, 5);
        assert!(!frame.is_well_formed());
        assert_eq!(frame.pixel(0, 0), None);
    }

    #[test]
    fn pixel_reads_rgba_in_row_major_order() {
        let mut bytes = vec![0u8; 2 * 2 * 4];
        bytes[4..8].copy_from_slice(&[10, 20, 30, 255]);
        let frame = frame(2, 2, bytes);
        assert_eq!(frame.pixel(0, 0), Some([0, 0, 0, 0]));
        assert_eq!(frame.pixel(1, 0), Some([10, 20, 30, 255]));
        assert_eq!(frame.pixel(2, 0), None);
        assert_eq!(frame.pixel(0, 2), None);
    }

    #[test]
    fn blank_means_all_zero_bytes() {
        assert!(frame(2, 2, vec![0; 16]).is_blank());
        let mut bytes = vec![0u8; 16];
        bytes[3] = 255;
        assert!(!frame(2, 2, bytes).is_blank());
    }

    #[test]
    fn padding_rows_do_not_make_a_frame_blank_or_offset_its_pixels() {
        let frame = padded(2, 2, 16);
        assert!(!frame.is_blank());
        assert_eq!(frame.pixel(0, 0), Some([10, 20, 30, 255]));
        assert_eq!(frame.pixel(1, 1), Some([10, 20, 30, 255]));
        assert_eq!(frame.tight_pixels(), [10, 20, 30, 255].repeat(4));
    }

    #[test]
    fn tight_pixels_strips_the_padding_between_rows() {
        let frame = padded(1, 2, 8);
        assert_eq!(frame.stride as usize, 8);
        assert_eq!(frame.tight_pixels(), vec![10, 20, 30, 255, 10, 20, 30, 255]);
    }

    #[test]
    fn a_malformed_frame_yields_zeroed_pixels_rather_than_panicking() {
        let frame = frame(2, 2, vec![7; 4]);
        assert!(!frame.is_well_formed());
        assert_eq!(frame.tight_pixels(), vec![0; 16]);
        assert!(!frame.is_blank());
    }

    #[test]
    fn pixels_outside_a_padded_frame_are_none() {
        let frame = padded(1, 1, 256);
        assert!(frame.pixel(1, 0).is_none());
        assert!(frame.pixel(0, 1).is_none());
    }

    #[test]
    fn a_cpu_frame_is_not_treated_as_bgra() {
        let frame = frame(1, 1, vec![10, 20, 30, 255]);
        assert!(!frame.is_bgra());
        assert_eq!(frame.pixel(0, 0), Some([10, 20, 30, 255]));
        assert_eq!(frame.tight_pixels(), vec![10, 20, 30, 255]);
    }

    #[test]
    fn a_bgra_picture_is_handed_out_as_rgba() {
        assert_eq!(to_rgba([30, 20, 10, 255], true), [10, 20, 30, 255]);
        assert_eq!(to_rgba([10, 20, 30, 255], false), [10, 20, 30, 255]);

        let mut bytes = vec![30, 20, 10, 255, 60, 50, 40, 128];
        swizzle_bgra_to_rgba(&mut bytes);
        assert_eq!(bytes, vec![10, 20, 30, 255, 40, 50, 60, 128]);
    }

    #[test]
    fn a_cpu_frame_lends_its_pixels_with_its_own_stride() {
        let frame = padded(1, 1, 16);
        let (len, stride, first) =
            frame.with_pixels(|bytes, stride| (bytes.len(), stride, bytes[0]));
        assert_eq!(len, 16);
        assert_eq!(stride, 16);
        assert_eq!(first, 10);
    }

    #[test]
    fn a_frame_can_cross_a_thread_boundary() {
        fn send_and_sync<T: Send + Sync>() {}
        send_and_sync::<Frame>();
        send_and_sync::<Arc<Frame>>();
    }

    #[test]
    fn half_precision_round_trips_the_values_a_surface_carries() {
        assert_eq!(half_to_f32(0x0000), 0.0);
        assert_eq!(half_to_f32(0x3c00), 1.0);
        assert_eq!(half_to_f32(0x3800), 0.5);
        assert_eq!(half_to_f32(0xbc00), -1.0);
        assert_eq!(half_to_f32(0x3555), 0.333_251_95);
        assert_eq!(half_to_f32(0x7c00), f32::INFINITY);
        assert!(half_to_f32(0x7e00).is_nan());
        assert_eq!(half_to_f32(f32_to_half(0.333_251_95)), 0.333_251_95);
        assert_eq!(half_to_f32(f32_to_half(1.751_953_1)), 1.751_953_1);
    }

    #[test]
    fn a_float_frame_reads_extended_values_and_clamps_its_eight_bit_view() {
        let frame = half_frame(
            3,
            1,
            &[
                [0.0, 0.25, 0.5, 1.0],
                [0.751_953_1, 2.5, 0.1, 1.0],
                [4.0, 0.0, 0.0, 1.0],
            ],
        );
        assert_eq!(frame.format, PixelFormat::Rgba16Float);
        assert!(frame.is_well_formed());
        assert_eq!(frame.extended_pixel(0, 0), Some([0.0, 0.25, 0.5, 1.0]));
        assert_eq!(frame.extended_pixel(1, 0).map(|p| p[1]), Some(2.5));
        assert_eq!(frame.extended_pixel(2, 0).map(|p| p[0]), Some(4.0));
        assert_eq!(frame.pixel(1, 0), Some([192, 255, 25, 255]));
        assert_eq!(frame.pixel(2, 0).map(|p| p[0]), Some(255));
        assert!(!frame.is_bgra());
        assert!(!frame.is_blank());
    }

    #[test]
    fn a_float_frame_states_its_own_precision() {
        assert_eq!(PixelFormat::Rgba16Float.bits_per_channel(), 16);
        assert_eq!(PixelFormat::Rgba16Float.bytes_per_pixel(), 8);
        assert!(PixelFormat::Rgba16Float.is_extended_range());
        assert_eq!(PixelFormat::Bgra8.bits_per_channel(), 8);
        assert_eq!(PixelFormat::Bgra8.bytes_per_pixel(), 4);
        assert!(!PixelFormat::Bgra8.is_extended_range());
    }

    #[test]
    fn the_default_output_description_is_the_shipped_eight_bit_path() {
        let output = OutputDescription::default();
        assert_eq!(output.format, PixelFormat::Bgra8);
        assert!(!output.extended_range_requested);
        assert!(!output.is_extended_range());
        assert_eq!(output.bits_per_channel(), 8);
        assert!(output.label().contains("8 bits per channel"));
        assert_eq!(output.transfer, OutputTransfer::Srgb);
        assert_eq!(output.tone_mapping, ToneMapping::MpvDefault);
    }

    #[test]
    fn an_extended_output_description_names_its_peak_and_passthrough() {
        let output = OutputDescription::extended_float(Some(1000));
        assert!(output.is_extended_range());
        assert_eq!(output.bits_per_channel(), 16);
        assert_eq!(output.transfer, OutputTransfer::Pq);
        assert_eq!(output.tone_mapping, ToneMapping::Passthrough);
        let label = output.label();
        assert!(label.contains("16 bits per channel"));
        assert!(label.contains("pq"));
        assert!(label.contains("target peak 1000 nits"));
        assert!(label.contains("none"));
    }

    #[cfg(target_os = "macos")]
    mod gpu {
        use super::*;

        fn surface() -> Arc<SurfaceFrame> {
            Arc::new(SurfaceFrame::new(32, 18).expect("a small surface exists"))
        }

        #[test]
        fn a_surface_frame_is_well_formed_and_measures_its_surface() {
            let surface = surface();
            let frame = Frame::gpu(32, 18, Arc::clone(&surface), 7);
            assert!(frame.is_well_formed());
            assert_eq!(frame.stride, surface.bytes_per_row());
            assert_eq!(frame.width, 32);
            assert_eq!(frame.height, 18);
            assert_eq!(frame.format, PixelFormat::Bgra8);
            assert!(frame.is_bgra());
        }

        #[test]
        fn a_surface_frame_reports_its_own_dimensions_as_the_validity_rule() {
            let frame = Frame::gpu(64, 18, surface(), 8);
            assert!(
                !frame.is_well_formed(),
                "the surface must be the size the frame claims"
            );
            assert_eq!(frame.pixel(0, 0), None);
            assert!(!Frame::gpu(0, 18, surface(), 9).is_well_formed());
        }

        #[test]
        fn a_fresh_surface_frame_is_blank_and_reads_zeroed_bgra() {
            let frame = Frame::gpu(32, 18, surface(), 10);
            assert!(frame.is_blank());
            assert_eq!(frame.pixel(16, 9), Some([0, 0, 0, 0]));
            assert_eq!(frame.pixel(32, 9), None);
            assert_eq!(frame.tight_pixels().len(), 32 * 4 * 18);
            let (len, stride) = frame.with_pixels(|bytes, stride| (bytes.len(), stride));
            assert_eq!(stride, frame.stride as usize);
            assert_eq!(len, stride * 18);
        }

        #[test]
        fn an_extended_surface_frame_describes_its_own_format() {
            let surface = Arc::new(
                SurfaceFrame::new_with_format(16, 9, PixelFormat::Rgba16Float)
                    .expect("a half-float surface exists"),
            );
            let frame = Frame::gpu(16, 9, Arc::clone(&surface), 11);
            assert_eq!(frame.format, PixelFormat::Rgba16Float);
            assert!(frame.is_well_formed());
            assert!(!frame.is_bgra());
            assert_eq!(frame.stride, surface.bytes_per_row());
            assert_eq!(frame.pixel(0, 0), Some([0, 0, 0, 0]));
            assert_eq!(frame.extended_pixel(0, 0), Some([0.0, 0.0, 0.0, 0.0]));
        }
    }
}
