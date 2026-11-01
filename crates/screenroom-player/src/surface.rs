use std::ffi::c_void;
use std::ptr;

use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString};
use objc2_io_surface::{
    IOSurfaceLockOptions, IOSurfaceRef, kIOSurfaceBytesPerElement, kIOSurfaceBytesPerRow,
    kIOSurfaceHeight, kIOSurfacePixelFormat, kIOSurfaceWidth,
};

use crate::PlayerError;

const BGRA: i32 = i32::from_be_bytes(*b"BGRA");
const BYTES_PER_ELEMENT: i32 = 4;

pub struct SurfaceFrame {
    surface: CFRetained<IOSurfaceRef>,

    width: u32,

    height: u32,

    bytes_per_row: u32,
}

unsafe impl Send for SurfaceFrame {}
unsafe impl Sync for SurfaceFrame {}

impl std::fmt::Debug for SurfaceFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SurfaceFrame")
            .field("size", &(self.width, self.height))
            .field("bytes_per_row", &self.bytes_per_row)
            .finish_non_exhaustive()
    }
}

impl SurfaceFrame {
    pub fn new(width: u32, height: u32) -> Result<Self, PlayerError> {
        if width == 0 || height == 0 {
            return Err(PlayerError::Argument(format!(
                "an IOSurface needs a non-zero size, got {width}x{height}"
            )));
        }
        let bytes_per_row = width.checked_mul(4).ok_or_else(|| {
            PlayerError::Argument(format!("{width} pixels do not fit into one surface row"))
        })?;
        let keys: [&CFString; 5] = unsafe {
            [
                kIOSurfaceWidth,
                kIOSurfaceHeight,
                kIOSurfaceBytesPerRow,
                kIOSurfaceBytesPerElement,
                kIOSurfacePixelFormat,
            ]
        };
        let width_number = CFNumber::new_i32(width as i32);
        let height_number = CFNumber::new_i32(height as i32);
        let row_number = CFNumber::new_i32(bytes_per_row as i32);
        let element_number = CFNumber::new_i32(BYTES_PER_ELEMENT);
        let format_number = CFNumber::new_i32(BGRA);
        let values: [&CFNumber; 5] = [
            &width_number,
            &height_number,
            &row_number,
            &element_number,
            &format_number,
        ];
        let properties = CFDictionary::<CFString, CFNumber>::from_slices(&keys, &values);
        let properties: CFRetained<CFDictionary> =
            unsafe { CFRetained::cast_unchecked(properties) };
        let surface = unsafe { IOSurfaceRef::new(&properties) }.ok_or_else(|| {
            PlayerError::Unavailable(format!("IOSurfaceCreate failed for {width}x{height}"))
        })?;
        let created_width = surface.width() as u32;
        let created_height = surface.height() as u32;
        let created_row = surface.bytes_per_row();
        if created_width != width || created_height != height || created_row < width as usize * 4 {
            return Err(PlayerError::Unavailable(format!(
                "the IOSurface came back as {created_width}x{created_height} with {created_row} bytes per row, not {width}x{height} with {bytes_per_row}"
            )));
        }
        Ok(Self {
            surface,
            width: created_width,
            height: created_height,
            bytes_per_row: created_row as u32,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn bytes_per_row(&self) -> u32 {
        self.bytes_per_row
    }

    pub fn as_ptr(&self) -> *mut c_void {
        CFRetained::as_ptr(&self.surface).as_ptr().cast()
    }

    pub fn base_address(&self) -> *mut u8 {
        self.surface.base_address().as_ptr().cast()
    }

    pub fn with_pixels<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        let _lock = SurfaceLock::acquire(&self.surface);
        let length = self.bytes_per_row as usize * self.height as usize;
        let bytes = unsafe { std::slice::from_raw_parts(self.base_address(), length) };
        f(bytes)
    }

    pub fn read_pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.with_pixels(|bytes| {
            let offset = y as usize * self.bytes_per_row as usize + x as usize * 4;
            bytes
                .get(offset..offset + 4)
                .map(|pixel| [pixel[0], pixel[1], pixel[2], pixel[3]])
        })
    }
}

struct SurfaceLock<'a> {
    surface: &'a IOSurfaceRef,

    locked: bool,
}

impl<'a> SurfaceLock<'a> {
    fn acquire(surface: &'a IOSurfaceRef) -> Self {
        let result = unsafe { surface.lock(IOSurfaceLockOptions::ReadOnly, ptr::null_mut()) };
        Self {
            surface,
            locked: result == 0,
        }
    }
}

impl Drop for SurfaceLock<'_> {
    fn drop(&mut self) {
        if self.locked {
            unsafe {
                self.surface
                    .unlock(IOSurfaceLockOptions::ReadOnly, ptr::null_mut())
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_surface_reports_its_size_and_row_stride() {
        let surface = SurfaceFrame::new(64, 36).expect("a 64x36 surface exists");
        assert_eq!(surface.width(), 64);
        assert_eq!(surface.height(), 36);
        assert!(surface.bytes_per_row() as usize >= 64 * 4);
        assert_eq!(surface.bytes_per_row() % 4, 0);
        assert!(!surface.as_ptr().is_null());
        assert!(!surface.base_address().is_null());
    }

    #[test]
    fn reading_a_surface_lends_a_locked_window_of_its_pixels() {
        let surface = SurfaceFrame::new(8, 4).expect("an 8x4 surface exists");
        let (length, base) = surface.with_pixels(|bytes| (bytes.len(), bytes.as_ptr()));
        assert_eq!(length, surface.bytes_per_row() as usize * 4);
        assert!(!base.is_null());
        let again = surface.with_pixels(|bytes| bytes.len());
        assert_eq!(again, length, "the surface unlocks between reads");
    }

    #[test]
    fn pixels_inside_the_surface_are_read_and_outside_are_none() {
        let surface = SurfaceFrame::new(8, 4).expect("an 8x4 surface exists");
        assert!(surface.read_pixel(0, 0).is_some());
        assert!(surface.read_pixel(7, 3).is_some());
        assert!(surface.read_pixel(8, 0).is_none());
        assert!(surface.read_pixel(0, 4).is_none());
    }

    #[test]
    fn a_zero_sized_surface_is_refused() {
        assert!(SurfaceFrame::new(0, 4).is_err());
        assert!(SurfaceFrame::new(4, 0).is_err());
    }

    #[test]
    fn debug_names_the_surface_and_its_stride() {
        let surface = SurfaceFrame::new(16, 9).expect("a 16x9 surface exists");
        let rendered = format!("{surface:?}");
        assert!(rendered.contains("SurfaceFrame"));
        assert!(rendered.contains("bytes_per_row"));
    }
}
