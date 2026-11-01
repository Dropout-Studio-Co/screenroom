use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Frame {
    pub width: u32,

    pub height: u32,

    pub stride: u32,

    pub rgba: Arc<Vec<u8>>,

    pub generation: u64,
}

impl Frame {
    pub fn is_well_formed(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.stride as usize >= self.width as usize * 4
            && self.rgba.len() == self.stride as usize * self.height as usize
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height || !self.is_well_formed() {
            return None;
        }
        let offset = y as usize * self.stride as usize + x as usize * 4;
        Some([
            self.rgba[offset],
            self.rgba[offset + 1],
            self.rgba[offset + 2],
            self.rgba[offset + 3],
        ])
    }

    pub fn is_blank(&self) -> bool {
        let row_bytes = self.width as usize * 4;
        let stride = self.stride as usize;
        (0..self.height as usize).all(|y| {
            let start = y * stride;
            self.rgba.get(start..start + row_bytes).is_some_and(|row| {
                row.as_chunks::<4>()
                    .0
                    .iter()
                    .all(|pixel| pixel[0] == 0 && pixel[1] == 0 && pixel[2] == 0 && pixel[3] == 0)
            })
        })
    }

    pub fn tight_pixels(&self) -> Vec<u8> {
        let row_bytes = self.width as usize * 4;
        let mut out = vec![0u8; row_bytes * self.height as usize];
        if !self.is_well_formed() {
            return out;
        }
        for (y, row) in out.chunks_exact_mut(row_bytes).enumerate() {
            let start = y * self.stride as usize;
            row.copy_from_slice(&self.rgba[start..start + row_bytes]);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: u32, height: u32, bytes: Vec<u8>) -> Frame {
        Frame {
            width,
            height,
            stride: width * 4,
            rgba: Arc::new(bytes),
            generation: 1,
        }
    }

    fn padded(width: u32, height: u32, stride: u32) -> Frame {
        let mut bytes = vec![0xAAu8; stride as usize * height as usize];
        for y in 0..height as usize {
            for x in 0..width as usize {
                let offset = y * stride as usize + x * 4;
                bytes[offset..offset + 4].copy_from_slice(&[10, 20, 30, 255]);
            }
        }
        Frame {
            width,
            height,
            stride,
            rgba: Arc::new(bytes),
            generation: 2,
        }
    }

    #[test]
    fn well_formed_checks_buffer_length() {
        assert!(frame(2, 2, vec![0; 16]).is_well_formed());
        assert!(!frame(2, 2, vec![0; 15]).is_well_formed());
        assert!(!frame(0, 2, vec![]).is_well_formed());
        assert!(padded(2, 2, 16).is_well_formed());
        let narrow = Frame {
            width: 2,
            height: 2,
            stride: 4,
            rgba: Arc::new(vec![0; 8]),
            generation: 3,
        };
        assert!(
            !narrow.is_well_formed(),
            "a row narrower than its pixels is invalid"
        );
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
}
