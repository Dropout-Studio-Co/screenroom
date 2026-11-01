use std::ffi::{c_char, c_int, c_void};
use std::ptr::{self, NonNull};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::surface::SurfaceFrame;
use crate::{Frame, LibMpv, Mpv, PlayerError, RenderSymbols, WakeSignal};

pub const UPDATE_FRAME: u64 = 1;

const SURFACE_POOL_SIZE: usize = 3;
const MAX_SURFACE_POOL_SIZE: usize = 8;

const PARAM_API_TYPE: c_int = 1;
const PARAM_OPENGL_INIT_PARAMS: c_int = 2;
const PARAM_OPENGL_FBO: c_int = 3;
const PARAM_FLIP_Y: c_int = 4;

const CGL_PFA_OPENGL_PROFILE: u32 = 99;
const CGL_PFA_ACCELERATED: u32 = 73;
const CGL_PFA_DOUBLE_BUFFER: u32 = 5;
const CGL_OPENGL_VERSION_3_2_CORE: u32 = 0x3200;

const GL_TEXTURE_RECTANGLE: u32 = 0x84f5;
const GL_RGBA8: u32 = 0x8058;
const GL_BGRA: u32 = 0x80e1;
const GL_UNSIGNED_INT_8_8_8_8_REV: u32 = 0x8367;
const GL_FRAMEBUFFER: u32 = 0x8d40;
const GL_COLOR_ATTACHMENT0: u32 = 0x8ce0;
const GL_FRAMEBUFFER_COMPLETE: u32 = 0x8cd5;

const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;

#[link(name = "OpenGL", kind = "framework")]
unsafe extern "C" {
    fn CGLChoosePixelFormat(
        attributes: *const u32,
        pixel_format: *mut *mut c_void,
        format_count: *mut i32,
    ) -> i32;
    fn CGLCreateContext(
        pixel_format: *mut c_void,
        share: *mut c_void,
        context: *mut *mut c_void,
    ) -> i32;
    fn CGLSetCurrentContext(context: *mut c_void) -> i32;
    fn CGLDestroyContext(context: *mut c_void) -> i32;
    fn CGLDestroyPixelFormat(pixel_format: *mut c_void) -> i32;
    #[allow(clippy::too_many_arguments)]
    fn CGLTexImageIOSurface2D(
        context: *mut c_void,
        target: u32,
        internal_format: u32,
        width: i32,
        height: i32,
        format: u32,
        kind: u32,
        surface: *mut c_void,
        plane: u32,
    ) -> i32;
    fn glGenTextures(count: i32, textures: *mut u32);
    fn glDeleteTextures(count: i32, textures: *const u32);
    fn glBindTexture(target: u32, texture: u32);
    fn glGenFramebuffers(count: i32, framebuffers: *mut u32);
    fn glDeleteFramebuffers(count: i32, framebuffers: *const u32);
    fn glBindFramebuffer(target: u32, framebuffer: u32);
    fn glFramebufferTexture2D(
        target: u32,
        attachment: u32,
        texture_target: u32,
        texture: u32,
        level: i32,
    );
    fn glCheckFramebufferStatus(target: u32) -> u32;
    fn glFinish();
}

unsafe extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

#[repr(C)]
struct RenderParam {
    kind: c_int,
    data: *mut c_void,
}

#[repr(C)]
struct OpenGlInitParams {
    get_proc_address: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
struct OpenGlFbo {
    fbo: c_int,
    w: c_int,
    h: c_int,
    internal_format: c_int,
}

unsafe extern "C" fn lookup_proc(_context: *mut c_void, name: *const c_char) -> *mut c_void {
    if name.is_null() {
        return ptr::null_mut();
    }
    unsafe { dlsym(RTLD_DEFAULT, name) }
}

struct RenderCallbackData {
    dirty: AtomicBool,
    wake: Arc<WakeSignal>,
}

unsafe extern "C" fn mark_dirty(context: *mut c_void) {
    if context.is_null() {
        return;
    }
    let callback_data = unsafe { &*(context as *const RenderCallbackData) };
    callback_data.dirty.store(true, Ordering::Release);
    callback_data.wake.notify();
}

struct CglContext {
    context: NonNull<c_void>,
    pixel_format: NonNull<c_void>,
}

impl CglContext {
    fn new() -> Result<Self, PlayerError> {
        let attributes = [
            CGL_PFA_OPENGL_PROFILE,
            CGL_OPENGL_VERSION_3_2_CORE,
            CGL_PFA_ACCELERATED,
            CGL_PFA_DOUBLE_BUFFER,
            0,
        ];
        let mut pixel_format: *mut c_void = ptr::null_mut();
        let mut format_count: c_int = 0;
        let code = unsafe {
            CGLChoosePixelFormat(attributes.as_ptr(), &mut pixel_format, &mut format_count)
        };
        let pixel_format = NonNull::new(pixel_format)
            .filter(|_| code == 0)
            .ok_or_else(|| {
                PlayerError::Unavailable(format!("CGLChoosePixelFormat failed with {code}"))
            })?;
        let mut context: *mut c_void = ptr::null_mut();
        let code =
            unsafe { CGLCreateContext(pixel_format.as_ptr(), ptr::null_mut(), &mut context) };
        let context = match NonNull::new(context).filter(|_| code == 0) {
            Some(context) => context,
            None => {
                unsafe { CGLDestroyPixelFormat(pixel_format.as_ptr()) };
                return Err(PlayerError::Unavailable(format!(
                    "CGLCreateContext failed with {code}"
                )));
            }
        };
        let cgl = Self {
            context,
            pixel_format,
        };
        cgl.make_current()?;
        Ok(cgl)
    }

    fn make_current(&self) -> Result<(), PlayerError> {
        let code = unsafe { CGLSetCurrentContext(self.context.as_ptr()) };
        if code != 0 {
            return Err(PlayerError::Unavailable(format!(
                "CGLSetCurrentContext failed with {code}"
            )));
        }
        Ok(())
    }

    fn handle(&self) -> *mut c_void {
        self.context.as_ptr()
    }
}

impl Drop for CglContext {
    fn drop(&mut self) {
        unsafe {
            CGLSetCurrentContext(ptr::null_mut());
            CGLDestroyContext(self.context.as_ptr());
            CGLDestroyPixelFormat(self.pixel_format.as_ptr());
        }
    }
}

struct Slot {
    surface: Arc<SurfaceFrame>,
    texture: u32,
    framebuffer: u32,
}

impl Slot {
    fn create(cgl: &CglContext, width: u32, height: u32) -> Result<Self, PlayerError> {
        let surface = Arc::new(SurfaceFrame::new(width, height)?);
        let mut texture: u32 = 0;
        unsafe { glGenTextures(1, &mut texture) };
        if texture == 0 {
            return Err(PlayerError::Unavailable(
                "glGenTextures returned no texture".to_string(),
            ));
        }
        unsafe { glBindTexture(GL_TEXTURE_RECTANGLE, texture) };
        let code = unsafe {
            CGLTexImageIOSurface2D(
                cgl.handle(),
                GL_TEXTURE_RECTANGLE,
                GL_RGBA8,
                width as i32,
                height as i32,
                GL_BGRA,
                GL_UNSIGNED_INT_8_8_8_8_REV,
                surface.as_ptr(),
                0,
            )
        };
        if code != 0 {
            unsafe { glDeleteTextures(1, &texture) };
            return Err(PlayerError::Unavailable(format!(
                "CGLTexImageIOSurface2D failed with {code}"
            )));
        }
        let mut framebuffer: u32 = 0;
        unsafe {
            glGenFramebuffers(1, &mut framebuffer);
            glBindFramebuffer(GL_FRAMEBUFFER, framebuffer);
            glFramebufferTexture2D(
                GL_FRAMEBUFFER,
                GL_COLOR_ATTACHMENT0,
                GL_TEXTURE_RECTANGLE,
                texture,
                0,
            );
        }
        if framebuffer == 0 {
            unsafe { glDeleteTextures(1, &texture) };
            return Err(PlayerError::Unavailable(
                "glGenFramebuffers returned no framebuffer".to_string(),
            ));
        }
        let status = unsafe { glCheckFramebufferStatus(GL_FRAMEBUFFER) };
        if status != GL_FRAMEBUFFER_COMPLETE {
            unsafe {
                glDeleteFramebuffers(1, &framebuffer);
                glDeleteTextures(1, &texture);
            }
            return Err(PlayerError::Unavailable(format!(
                "the surface framebuffer is incomplete (status {status:#x})"
            )));
        }
        Ok(Self {
            surface,
            texture,
            framebuffer,
        })
    }

    fn destroy(&self) {
        unsafe {
            glDeleteFramebuffers(1, &self.framebuffer);
            glDeleteTextures(1, &self.texture);
        }
    }
}

impl std::fmt::Debug for Slot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Slot")
            .field("size", &(self.surface.width(), self.surface.height()))
            .field("texture", &self.texture)
            .field("framebuffer", &self.framebuffer)
            .finish_non_exhaustive()
    }
}

pub struct GlRenderer {
    library: Arc<LibMpv>,
    symbols: RenderSymbols,
    context: NonNull<c_void>,
    callback_data: Box<RenderCallbackData>,
    cgl: CglContext,
    slots: Vec<Slot>,
    width: u32,
    height: u32,
    generation: u64,
    skipped: u64,
    target_failure: Option<String>,
}

impl std::fmt::Debug for GlRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlRenderer")
            .field("size", &(self.width, self.height))
            .field("slots", &self.slots)
            .field("generation", &self.generation)
            .field("skipped", &self.skipped)
            .finish_non_exhaustive()
    }
}

pub fn clamp_target(width: u32, height: u32) -> (u32, u32) {
    (width.max(1), height.max(1))
}

fn free_slot<T>(slots: &[T], shared: impl Fn(&T) -> usize) -> Option<usize> {
    slots.iter().position(|slot| shared(slot) == 1)
}

impl GlRenderer {
    pub fn new(core: &Mpv) -> Result<Self, PlayerError> {
        Self::with_wake(core, Arc::new(WakeSignal::new()))
    }

    pub(crate) fn with_wake(core: &Mpv, wake: Arc<WakeSignal>) -> Result<Self, PlayerError> {
        let library = Arc::clone(core.library());
        let symbols = library.render_symbols().ok_or_else(|| {
            PlayerError::Unavailable(format!(
                "{} does not export the libmpv render API",
                library.path()
            ))
        })?;
        let cgl = CglContext::new()?;
        let mut callback_data = Box::new(RenderCallbackData {
            dirty: AtomicBool::new(false),
            wake,
        });
        let callback_pointer = (&mut *callback_data as *mut RenderCallbackData).cast::<c_void>();

        let api_type = c"opengl";
        let mut init = OpenGlInitParams {
            get_proc_address: lookup_proc,
            get_proc_address_ctx: ptr::null_mut(),
        };
        let mut params = [
            RenderParam {
                kind: PARAM_API_TYPE,
                data: api_type.as_ptr() as *mut c_void,
            },
            RenderParam {
                kind: PARAM_OPENGL_INIT_PARAMS,
                data: (&mut init as *mut OpenGlInitParams).cast(),
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
                context: "mpv_render_context_create(opengl)".to_string(),
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
            symbols,
            context,
            callback_data,
            cgl,
            slots: Vec::new(),
            width: 0,
            height: 0,
            generation: 0,
            skipped: 0,
            target_failure: None,
        })
    }

    pub fn take_dirty(&self) -> bool {
        self.callback_data.dirty.swap(false, Ordering::AcqRel)
    }

    pub fn update_flags(&self) -> Result<u64, PlayerError> {
        Ok(unsafe { (self.symbols.update)(self.context.as_ptr()) })
    }

    pub fn set_target(&mut self, width: u32, height: u32) {
        let (width, height) = clamp_target(width, height);
        if (self.width, self.height) == (width, height)
            && !self.slots.is_empty()
            && self.target_failure.is_none()
        {
            return;
        }
        if self.cgl.make_current().is_err() {
            return;
        }
        self.width = width;
        self.height = height;
        self.target_failure = None;
        self.release_slots();
        let mut slots = Vec::with_capacity(SURFACE_POOL_SIZE);
        for _ in 0..SURFACE_POOL_SIZE {
            match Slot::create(&self.cgl, width, height) {
                Ok(slot) => slots.push(slot),
                Err(error) => {
                    self.target_failure = Some(error.to_string());
                    break;
                }
            }
        }
        self.slots = slots;
    }

    pub fn skipped(&self) -> u64 {
        self.skipped
    }

    pub fn render(&mut self) -> Result<Option<Frame>, PlayerError> {
        if self.width == 0 || self.height == 0 {
            return Ok(None);
        }
        if let Some(reason) = &self.target_failure {
            return Err(PlayerError::Unavailable(format!(
                "the OpenGL surface pool is not ready: {reason}"
            )));
        }
        self.cgl.make_current()?;
        self.reap_surplus_slots();
        let index = match free_slot(&self.slots, |slot| Arc::strong_count(&slot.surface)) {
            Some(index) => index,
            None if self.slots.len() < MAX_SURFACE_POOL_SIZE => {
                match Slot::create(&self.cgl, self.width, self.height) {
                    Ok(slot) => {
                        self.slots.push(slot);
                        self.slots.len() - 1
                    }
                    Err(error) => {
                        self.target_failure = Some(error.to_string());
                        return Err(error);
                    }
                }
            }
            None => {
                self.skipped += 1;
                return Ok(None);
            }
        };
        let framebuffer = self.slots[index].framebuffer;
        let mut fbo = OpenGlFbo {
            fbo: framebuffer as c_int,
            w: self.width as c_int,
            h: self.height as c_int,
            internal_format: GL_RGBA8 as c_int,
        };
        let mut flip_y: c_int = 0;
        let mut params = [
            RenderParam {
                kind: PARAM_OPENGL_FBO,
                data: (&mut fbo as *mut OpenGlFbo).cast(),
            },
            RenderParam {
                kind: PARAM_FLIP_Y,
                data: (&mut flip_y as *mut c_int).cast(),
            },
            RenderParam {
                kind: 0,
                data: ptr::null_mut(),
            },
        ];

        self.generation += 1;
        unsafe { glBindFramebuffer(GL_FRAMEBUFFER, framebuffer) };
        let began = std::time::Instant::now();
        let code =
            unsafe { (self.symbols.render)(self.context.as_ptr(), params.as_mut_ptr().cast()) };
        unsafe { glFinish() };
        let drawn = began.elapsed();
        if code < 0 {
            return Err(PlayerError::Mpv {
                context: "mpv_render_context_render".to_string(),
                message: self.library.message(code),
                code,
            });
        }
        if std::env::var_os("SCREENROOM_PLAYER_TRACE").is_some() {
            eprintln!(
                "[gl_render] {}x{} slot {} — libmpv {:?}, glFinish included",
                self.width, self.height, index, drawn
            );
        }
        let surface = Arc::clone(&self.slots[index].surface);
        if let Some(report_swap) = self.symbols.report_swap {
            unsafe { report_swap(self.context.as_ptr()) };
        }
        Ok(Some(Frame::gpu(
            self.width,
            self.height,
            surface,
            self.generation,
        )))
    }

    fn reap_surplus_slots(&mut self) {
        while self.slots.len() > SURFACE_POOL_SIZE {
            let Some(index) = self
                .slots
                .iter()
                .rposition(|slot| Arc::strong_count(&slot.surface) == 1)
            else {
                return;
            };
            let slot = self.slots.remove(index);
            slot.destroy();
        }
    }

    #[allow(dead_code)]
    pub fn raw_context(&self) -> *mut c_void {
        self.context.as_ptr()
    }

    fn release_slots(&mut self) {
        for slot in self.slots.drain(..) {
            slot.destroy();
        }
    }
}

impl Drop for GlRenderer {
    fn drop(&mut self) {
        unsafe {
            (self.symbols.set_update_callback)(self.context.as_ptr(), None, ptr::null_mut());
            (self.symbols.free)(self.context.as_ptr());
        }
        if self.cgl.make_current().is_ok() {
            self.release_slots();
        }
        self.callback_data.dirty.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_free_slot_is_the_first_surface_nobody_holds() {
        let slots: Vec<Arc<u32>> = (0..SURFACE_POOL_SIZE as u32).map(Arc::new).collect();
        let shared = |slot: &Arc<u32>| Arc::strong_count(slot);
        assert_eq!(free_slot(&slots, shared), Some(0));

        let held = Arc::clone(&slots[0]);
        assert_eq!(free_slot(&slots, shared), Some(1));
        drop(held);
        assert_eq!(free_slot(&slots, shared), Some(0));
    }

    #[test]
    fn a_pool_whose_surfaces_are_all_held_offers_nothing() {
        let slots: Vec<Arc<u32>> = (0..SURFACE_POOL_SIZE as u32).map(Arc::new).collect();
        let shared = |slot: &Arc<u32>| Arc::strong_count(slot);
        let held: Vec<Arc<u32>> = slots.iter().map(Arc::clone).collect();
        assert_eq!(free_slot(&slots, shared), None);
        drop(held);
        assert_eq!(free_slot(&slots, shared), Some(0));
    }

    #[test]
    fn an_empty_pool_offers_nothing() {
        let slots: Vec<Arc<u32>> = Vec::new();
        assert_eq!(free_slot(&slots, Arc::strong_count), None);
    }

    #[test]
    fn a_target_is_never_zero_sized() {
        assert_eq!(clamp_target(0, 0), (1, 1));
        assert_eq!(clamp_target(0, 1080), (1, 1080));
        assert_eq!(clamp_target(3840, 0), (3840, 1));
        assert_eq!(clamp_target(3840, 2160), (3840, 2160));
    }

    #[test]
    fn the_pool_is_three_surfaces_deep() {
        assert_eq!(SURFACE_POOL_SIZE, 3);
    }
}
