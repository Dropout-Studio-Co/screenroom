use std::ffi::{c_int, c_void};
use std::ptr::{self, NonNull};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{Frame, LibMpv, PlayerError};

pub const UPDATE_FRAME: u64 = 1;

pub const MAX_DIMENSION: u32 = 3840;
const FRAME_BUFFER_POOL_SIZE: usize = 3;
const STRIDE_ALIGNMENT: usize = 256;

const PARAM_API_TYPE: c_int = 1;
const PARAM_SW_SIZE: c_int = 17;
const PARAM_SW_FORMAT: c_int = 18;
const PARAM_SW_STRIDE: c_int = 19;
const PARAM_SW_POINTER: c_int = 20;

#[repr(C)]
struct RenderParam {
    kind: c_int,
    data: *mut c_void,
}

pub fn target_size(video_width: u32, video_height: u32, max_dimension: u32) -> (u32, u32) {
    let longest = video_width.max(video_height);
    if video_width == 0 || video_height == 0 {
        return (1, 1);
    }
    if longest <= max_dimension {
        return (video_width, video_height);
    }
    let scale = max_dimension as f64 / longest as f64;
    let scale_axis = |value: u32| ((value as f64 * scale).round() as u32).max(1);
    (scale_axis(video_width), scale_axis(video_height))
}

fn row_stride(width: u32) -> usize {
    let bytes = width.max(1) as usize * 4;
    bytes.div_ceil(STRIDE_ALIGNMENT) * STRIDE_ALIGNMENT
}

pub struct SoftwareRenderer {
    library: Arc<LibMpv>,
    context: NonNull<c_void>,

    callback_data: Box<RenderCallbackData>,
    width: u32,
    height: u32,
    stride: usize,
    buffer: Vec<u8>,
    offset: usize,
    packed_buffers: Vec<Arc<Vec<u8>>>,
    generation: u64,
}

struct RenderCallbackData {
    dirty: AtomicBool,
    wake: Arc<crate::WakeSignal>,
}

impl std::fmt::Debug for SoftwareRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SoftwareRenderer")
            .field("size", &(self.width, self.height))
            .field("stride", &self.stride)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

unsafe extern "C" fn mark_dirty(context: *mut c_void) {
    if context.is_null() {
        return;
    }

    let callback_data = unsafe { &*(context as *const RenderCallbackData) };
    callback_data.dirty.store(true, Ordering::Release);
    callback_data.wake.notify();
}

impl SoftwareRenderer {
    pub fn new(core: &crate::Mpv, wake: Arc<crate::WakeSignal>) -> Result<Self, PlayerError> {
        let library = Arc::clone(core.library());
        let symbols = library.render_symbols().ok_or_else(|| {
            PlayerError::Unavailable(format!(
                "{} does not export the libmpv render API",
                library.path()
            ))
        })?;

        let mut callback_data = Box::new(RenderCallbackData {
            dirty: AtomicBool::new(false),
            wake,
        });
        let callback_pointer = (&mut *callback_data as *mut RenderCallbackData).cast::<c_void>();

        let api_type = c"sw";
        let mut params = [
            RenderParam {
                kind: PARAM_API_TYPE,
                data: api_type.as_ptr() as *mut c_void,
            },
            RenderParam {
                kind: 0,
                data: ptr::null_mut(),
            },
        ];
        let mut context: *mut c_void = ptr::null_mut();
        let code = unsafe {
            (symbols.create)(&mut context, core.raw_handle(), params.as_mut_ptr().cast())
        };
        if code < 0 {
            return Err(PlayerError::Mpv {
                context: "mpv_render_context_create(sw)".to_string(),
                message: library.message(code),
                code,
            });
        }
        let context = NonNull::new(context).ok_or_else(|| {
            PlayerError::Unavailable("mpv_render_context_create returned null".into())
        })?;

        unsafe {
            (symbols.set_update_callback)(context.as_ptr(), Some(mark_dirty), callback_pointer)
        };

        Ok(Self {
            library,
            context,
            callback_data,
            width: 0,
            height: 0,
            stride: 0,
            buffer: Vec::new(),
            offset: 0,
            packed_buffers: std::iter::repeat_with(|| Arc::new(Vec::new()))
                .take(FRAME_BUFFER_POOL_SIZE)
                .collect(),
            generation: 0,
        })
    }

    pub fn take_dirty(&self) -> bool {
        self.callback_data.dirty.swap(false, Ordering::AcqRel)
    }

    pub fn update_flags(&self) -> Result<u64, PlayerError> {
        let symbols = self.symbols()?;
        Ok(unsafe { (symbols.update)(self.context.as_ptr()) })
    }

    pub fn set_target(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        if (self.width, self.height) == (width, height) {
            return;
        }
        self.width = width;
        self.height = height;
        self.stride = row_stride(width);
        let needed = self.stride * height as usize + 64;
        if self.buffer.len() != needed {
            self.buffer = vec![0u8; needed];
        }
        let base = self.buffer.as_ptr() as usize;
        self.offset = (64 - (base % 64)) % 64;
        let frame_bytes = self.stride * height as usize;
        for buffer in &mut self.packed_buffers {
            if let Some(buffer) = Arc::get_mut(buffer)
                && buffer.len() != frame_bytes
            {
                buffer.resize(frame_bytes, 0);
            }
        }
    }

    pub fn render(&mut self) -> Result<Option<Frame>, PlayerError> {
        if self.width == 0 || self.height == 0 {
            return Ok(None);
        }
        let symbols = self.symbols()?;

        let mut size = [self.width as c_int, self.height as c_int];
        let format = c"rgb0";
        let stride = self.stride;

        let pointer = self.aligned_ptr();
        let mut params = [
            RenderParam {
                kind: PARAM_SW_SIZE,
                data: size.as_mut_ptr().cast(),
            },
            RenderParam {
                kind: PARAM_SW_FORMAT,
                data: format.as_ptr() as *mut c_void,
            },
            RenderParam {
                kind: PARAM_SW_STRIDE,
                data: (&stride as *const usize as *mut usize).cast(),
            },
            RenderParam {
                kind: PARAM_SW_POINTER,
                data: pointer.cast(),
            },
            RenderParam {
                kind: 0,
                data: ptr::null_mut(),
            },
        ];

        let began = std::time::Instant::now();
        let code = unsafe { (symbols.render)(self.context.as_ptr(), params.as_mut_ptr().cast()) };
        let drawn = began.elapsed();
        if code < 0 {
            return Err(PlayerError::Mpv {
                context: "mpv_render_context_render".to_string(),
                message: self.library.message(code),
                code,
            });
        }

        self.generation += 1;
        let began = std::time::Instant::now();
        let frame = self.pack(pointer);
        if std::env::var_os("SCREENROOM_PLAYER_TRACE").is_some() {
            eprintln!(
                "[render] {}x{} stride {} align {} — libmpv {:?}, pack {:?}",
                self.width,
                self.height,
                self.stride,
                (pointer as usize) % 64,
                drawn,
                began.elapsed()
            );
        }
        Ok(Some(frame))
    }

    #[allow(dead_code)]
    pub fn raw_context(&self) -> *mut c_void {
        self.context.as_ptr()
    }

    fn symbols(&self) -> Result<crate::RenderSymbols, PlayerError> {
        self.library.render_symbols().ok_or_else(|| {
            PlayerError::Unavailable(format!(
                "{} does not export the libmpv render API",
                self.library.path()
            ))
        })
    }

    fn aligned_ptr(&mut self) -> *mut u8 {
        unsafe { self.buffer.as_mut_ptr().add(self.offset) }
    }

    fn pack(&mut self, _pointer: *mut u8) -> Frame {
        let height = self.height as usize;
        let stride = self.stride;
        let needed = stride * height;
        let start = self.offset;
        let source = &self.buffer[start..start + needed];
        let output_index = reusable_buffer_index(&self.packed_buffers);
        let mut overflow = output_index.is_none().then(|| Arc::new(Vec::new()));
        {
            let rgba = match output_index {
                Some(index) => Arc::get_mut(&mut self.packed_buffers[index])
                    .expect("the selected frame buffer is uniquely owned"),
                None => Arc::get_mut(overflow.as_mut().expect("overflow buffer exists"))
                    .expect("the overflow frame buffer is uniquely owned"),
            };
            rgba.resize(needed, 0);
            rgba.as_mut_slice().copy_from_slice(source);
        }
        let rgba = output_index
            .map(|index| Arc::clone(&self.packed_buffers[index]))
            .unwrap_or_else(|| overflow.take().expect("overflow buffer exists"));

        Frame {
            width: self.width,
            height: self.height,
            stride: stride as u32,
            rgba,
            generation: self.generation,
        }
    }
}

fn reusable_buffer_index(buffers: &[Arc<Vec<u8>>]) -> Option<usize> {
    buffers
        .iter()
        .position(|buffer| Arc::strong_count(buffer) == 1)
}

impl Drop for SoftwareRenderer {
    fn drop(&mut self) {
        if let Some(symbols) = self.library.render_symbols() {
            unsafe { (symbols.set_update_callback)(self.context.as_ptr(), None, ptr::null_mut()) };
            unsafe { (symbols.free)(self.context.as_ptr()) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_sources_keep_their_size() {
        assert_eq!(target_size(1920, 1080, MAX_DIMENSION), (1920, 1080));
        assert_eq!(target_size(640, 480, MAX_DIMENSION), (640, 480));
    }

    #[test]
    fn four_k_sources_keep_their_own_resolution() {
        assert_eq!(target_size(3840, 2160, MAX_DIMENSION), (3840, 2160));
        assert_eq!(target_size(3240, 2160, MAX_DIMENSION), (3240, 2160));
        assert_eq!(target_size(4096, 2160, MAX_DIMENSION), (3840, 2025));
    }

    #[test]
    fn sources_beyond_four_k_are_downscaled_without_distortion() {
        assert_eq!(target_size(7680, 4320, MAX_DIMENSION), (3840, 2160));
        assert_eq!(target_size(4320, 7680, MAX_DIMENSION), (2160, 3840));
        let (width, height) = target_size(8192, 3432, MAX_DIMENSION);
        assert_eq!(width, 3840);
        assert!(height <= MAX_DIMENSION);
        let source_aspect = 8192.0 / 3432.0;
        let target_aspect = width as f64 / height as f64;
        assert!((source_aspect - target_aspect).abs() < 0.01);
    }

    #[test]
    fn degenerate_sizes_never_produce_zero() {
        assert_eq!(target_size(0, 0, MAX_DIMENSION), (1, 1));
        assert_eq!(target_size(0, 1080, MAX_DIMENSION), (1, 1));
        assert_eq!(target_size(100, 100, 0), (1, 1));
    }

    #[test]
    fn a_card_preview_target_is_bounded_to_the_card() {
        assert_eq!(target_size(3840, 2160, 960), (960, 540));
        assert_eq!(target_size(3240, 2160, 960), (960, 640));
    }

    #[test]
    fn pooled_buffers_can_be_sized_before_the_first_frame() {
        let mut buffers: Vec<Arc<Vec<u8>>> = (0..FRAME_BUFFER_POOL_SIZE)
            .map(|_| Arc::new(Vec::new()))
            .collect();
        let bytes = row_stride(3840) * 2160;
        for buffer in &mut buffers {
            if let Some(buffer) = Arc::get_mut(buffer)
                && buffer.len() != bytes
            {
                buffer.resize(bytes, 0);
            }
        }
        assert!(buffers.iter().all(|buffer| buffer.len() == bytes));
        assert_eq!(reusable_buffer_index(&buffers), Some(0));
    }

    #[test]
    fn row_stride_is_ready_for_a_direct_texture_upload() {
        for width in [1u32, 17, 1280, 1919, 1920, 3240, 3840] {
            let stride = row_stride(width);
            assert_eq!(stride % 256, 0, "wgpu copies need 256-byte rows");
            assert!(stride >= width as usize * 4);
            assert!(stride - width as usize * 4 < 256);
        }
        assert_eq!(row_stride(3840), 15360);
        assert_eq!(row_stride(3240), 13056);
    }

    #[test]
    fn packed_buffers_are_reused_after_the_frame_is_released() {
        let buffers = vec![Arc::new(vec![0; 32])];
        let original = buffers[0].as_ptr();
        let frame = Arc::clone(&buffers[0]);
        assert_eq!(reusable_buffer_index(&buffers), None);
        drop(frame);
        assert_eq!(reusable_buffer_index(&buffers), Some(0));
        assert_eq!(buffers[0].as_ptr(), original);
    }
}
