use std::ffi::{c_char, c_int, c_void};
use std::ptr::{self, NonNull};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::display::DisplayTarget;
use crate::frame::{OutputDescription, PixelFormat};
use crate::surface::SurfaceFrame;
use crate::{Frame, LibMpv, Mpv, PlayerError, RenderSymbols, WakeSignal};

pub const UPDATE_FRAME: u64 = 1;

const SURFACE_POOL_SIZE: usize = 3;
const MAX_SURFACE_POOL_SIZE: usize = 16;

const FENCE_TIMEOUT: Duration = Duration::from_secs(1);

const READER_GRACE: Duration = Duration::from_millis(250);

const PARAM_API_TYPE: c_int = 1;
const PARAM_OPENGL_INIT_PARAMS: c_int = 2;
const PARAM_OPENGL_FBO: c_int = 3;
const PARAM_FLIP_Y: c_int = 4;
const PARAM_DEPTH: c_int = 5;

const CGL_PFA_OPENGL_PROFILE: u32 = 99;
const CGL_PFA_ACCELERATED: u32 = 73;
const CGL_PFA_DOUBLE_BUFFER: u32 = 5;
const CGL_OPENGL_VERSION_3_2_CORE: u32 = 0x3200;

const GL_TEXTURE_RECTANGLE: u32 = 0x84f5;
const GL_RGBA8: u32 = 0x8058;
const GL_RGBA16F: u32 = 0x881a;
const GL_RGBA: u32 = 0x1908;
const GL_BGRA: u32 = 0x80e1;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
const GL_UNSIGNED_INT_8_8_8_8_REV: u32 = 0x8367;
const GL_HALF_FLOAT: u32 = 0x140b;
const GL_FRAMEBUFFER: u32 = 0x8d40;
const GL_COLOR_ATTACHMENT0: u32 = 0x8ce0;
const GL_FRAMEBUFFER_COMPLETE: u32 = 0x8cd5;

const GL_SYNC_GPU_COMMANDS_COMPLETE: u32 = 0x9117;

const GL_SYNC_FLUSH_COMMANDS_BIT: u32 = 0x0001;
const GL_ALREADY_SIGNALED: u32 = 0x911a;
const GL_TIMEOUT_EXPIRED: u32 = 0x911b;
const GL_CONDITION_SATISFIED: u32 = 0x911c;
const GL_WAIT_FAILED: u32 = 0x911d;

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
    fn glFenceSync(condition: u32, flags: u32) -> *mut c_void;
    fn glClientWaitSync(sync: *mut c_void, flags: u32, timeout: u64) -> u32;
    fn glDeleteSync(sync: *mut c_void);
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

fn gl_formats(format: PixelFormat) -> (u32, u32, u32) {
    match format {
        PixelFormat::Bgra8 => (GL_RGBA8, GL_BGRA, GL_UNSIGNED_INT_8_8_8_8_REV),
        PixelFormat::Rgba16Float => (GL_RGBA16F, GL_RGBA, GL_HALF_FLOAT),
        PixelFormat::Rgba8 => (GL_RGBA8, GL_RGBA, GL_UNSIGNED_BYTE),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FenceOutcome {
    Signalled,
    Waiting,
    Failed,
}

struct GlFence {
    handle: NonNull<c_void>,
}

impl GlFence {
    fn record() -> Option<Self> {
        NonNull::new(unsafe { glFenceSync(GL_SYNC_GPU_COMMANDS_COMPLETE, 0) })
            .map(|handle| Self { handle })
    }

    fn probe(&self) -> FenceOutcome {
        FenceOutcome::from_code(unsafe {
            glClientWaitSync(self.handle.as_ptr(), GL_SYNC_FLUSH_COMMANDS_BIT, 0)
        })
    }

    fn wait(&self, timeout: Duration) -> FenceOutcome {
        FenceOutcome::from_code(unsafe {
            glClientWaitSync(
                self.handle.as_ptr(),
                GL_SYNC_FLUSH_COMMANDS_BIT,
                timeout.as_nanos().min(u64::MAX as u128) as u64,
            )
        })
    }
}

impl FenceOutcome {
    fn from_code(code: u32) -> Self {
        match code {
            GL_ALREADY_SIGNALED | GL_CONDITION_SATISFIED => Self::Signalled,
            GL_TIMEOUT_EXPIRED => Self::Waiting,
            GL_WAIT_FAILED => Self::Failed,
            _ => Self::Failed,
        }
    }
}

impl Drop for GlFence {
    fn drop(&mut self) {
        unsafe { glDeleteSync(self.handle.as_ptr()) };
    }
}

impl std::fmt::Debug for GlFence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlFence").finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SlotWrite {
    generation: u64,
    pending: bool,
}

impl SlotWrite {
    fn begin(&mut self, generation: u64) {
        self.generation = generation;
        self.pending = true;
    }

    fn settle(&mut self) {
        self.pending = false;
    }
}

pub fn target_internal_format(format: PixelFormat) -> u32 {
    gl_formats(format).0
}

struct Slot {
    surface: Arc<SurfaceFrame>,
    texture: u32,
    framebuffer: u32,
    write: SlotWrite,
    fence: Option<GlFence>,
    published_at: Option<Instant>,
}

impl Slot {
    fn create(
        cgl: &CglContext,
        width: u32,
        height: u32,
        format: PixelFormat,
    ) -> Result<Self, PlayerError> {
        let surface = Arc::new(SurfaceFrame::new_with_format(width, height, format)?);
        let (internal_format, channels, samples) = gl_formats(format);
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
                internal_format,
                width as i32,
                height as i32,
                channels,
                samples,
                surface.as_ptr().cast(),
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
            write: SlotWrite::default(),
            fence: None,
            published_at: None,
        })
    }

    fn writable_at(&self, now: Instant) -> bool {
        Arc::strong_count(&self.surface) == 1 && !self.write.pending && self.reads_settled(now)
    }

    fn reads_reported(&self) -> bool {
        !self.surface.reads().outstanding()
    }

    fn reads_settled(&self, now: Instant) -> bool {
        if self.reads_reported() {
            return true;
        }
        self.published_at
            .is_some_and(|published| now.saturating_duration_since(published) >= READER_GRACE)
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
            .field("write", &self.write)
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
    fence_timeouts: u64,
    reclaimed_while_reading: u64,
    output: OutputDescription,
    target_failure: Option<String>,
}

impl std::fmt::Debug for GlRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlRenderer")
            .field("size", &(self.width, self.height))
            .field("slots", &self.slots)
            .field("generation", &self.generation)
            .field("skipped", &self.skipped)
            .field("fence_timeouts", &self.fence_timeouts)
            .field("reclaimed_while_reading", &self.reclaimed_while_reading)
            .finish_non_exhaustive()
    }
}

pub fn clamp_target(width: u32, height: u32) -> (u32, u32) {
    (width.max(1), height.max(1))
}

fn extended_range_values(target: DisplayTarget) -> [(&'static str, String); 5] {
    [
        ("target-prim", "bt.709".to_string()),
        ("target-trc", "pq".to_string()),
        ("target-peak", target.peak_nits().to_string()),
        ("tone-mapping", "bt.2390".to_string()),
        ("hdr-compute-peak", "no".to_string()),
    ]
}

fn extended_range_reset_values() -> [(&'static str, &'static str); 5] {
    [
        ("target-prim", "auto"),
        ("target-trc", "auto"),
        ("target-peak", "auto"),
        ("tone-mapping", "auto"),
        ("hdr-compute-peak", "auto"),
    ]
}

pub fn enable_extended_range(
    core: &Mpv,
    target: DisplayTarget,
) -> Result<OutputDescription, PlayerError> {
    for (name, value) in extended_range_values(target) {
        core.set_property(name, &value)?;
    }
    Ok(OutputDescription::extended_float(Some(target.peak_nits())))
}

pub fn reset_extended_range(core: &Mpv) -> Result<(), PlayerError> {
    for (name, value) in extended_range_reset_values() {
        core.set_property(name, value)?;
    }
    Ok(())
}

fn writable_slot(slots: &[Slot], now: Instant) -> Option<usize> {
    slots.iter().position(|slot| slot.writable_at(now))
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
            fence_timeouts: 0,
            reclaimed_while_reading: 0,
            output: OutputDescription::sdr_bgra8(),
            target_failure: None,
        })
    }

    pub fn take_dirty(&self) -> bool {
        self.callback_data.dirty.swap(false, Ordering::AcqRel)
    }

    pub fn update_flags(&self) -> Result<u64, PlayerError> {
        Ok(unsafe { (self.symbols.update)(self.context.as_ptr()) })
    }

    pub fn output(&self) -> &OutputDescription {
        &self.output
    }

    pub fn set_output(&mut self, output: OutputDescription) {
        if self.output.format == output.format {
            self.output = output;
            return;
        }
        self.output = output;
        self.target_failure = None;
        if self.cgl.make_current().is_ok() {
            self.release_slots();
        }
        self.width = 0;
        self.height = 0;
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
            match Slot::create(&self.cgl, width, height, self.output.format) {
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

    pub fn fence_timeouts(&self) -> u64 {
        self.fence_timeouts
    }

    pub fn reclaimed_while_reading(&self) -> u64 {
        self.reclaimed_while_reading
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
        self.settle_fences();
        self.reap_surplus_slots();
        let index = match writable_slot(&self.slots, Instant::now()) {
            Some(index) => index,
            None if self.slots.len() < MAX_SURFACE_POOL_SIZE => {
                match Slot::create(&self.cgl, self.width, self.height, self.output.format) {
                    Ok(slot) => {
                        self.slots.push(slot);
                        self.slots.len() - 1
                    }
                    Err(_) => {
                        self.skipped += 1;
                        return Ok(None);
                    }
                }
            }
            None => {
                self.skipped += 1;
                return Ok(None);
            }
        };
        let framebuffer = self.slots[index].framebuffer;
        if !self.slots[index].reads_reported() {
            self.reclaimed_while_reading += 1;
        }
        let (internal_format, _, _) = gl_formats(self.output.format);
        let mut fbo = OpenGlFbo {
            fbo: framebuffer as c_int,
            w: self.width as c_int,
            h: self.height as c_int,
            internal_format: internal_format as c_int,
        };
        let mut flip_y: c_int = 0;
        let mut depth: c_int = if self.output.format.is_extended_range() {
            c_int::from(self.output.format.bits_per_channel())
        } else {
            0
        };
        let mut params = vec![
            RenderParam {
                kind: PARAM_OPENGL_FBO,
                data: (&mut fbo as *mut OpenGlFbo).cast(),
            },
            RenderParam {
                kind: PARAM_FLIP_Y,
                data: (&mut flip_y as *mut c_int).cast(),
            },
            RenderParam {
                kind: PARAM_DEPTH,
                data: (&mut depth as *mut c_int).cast(),
            },
            RenderParam {
                kind: 0,
                data: ptr::null_mut(),
            },
        ];

        self.generation += 1;
        let generation = self.generation;
        unsafe { glBindFramebuffer(GL_FRAMEBUFFER, framebuffer) };
        let began = Instant::now();
        let code =
            unsafe { (self.symbols.render)(self.context.as_ptr(), params.as_mut_ptr().cast()) };
        let drawn = began.elapsed();
        if code < 0 {
            return Err(PlayerError::Mpv {
                context: "mpv_render_context_render".to_string(),
                message: self.library.message(code),
                code,
            });
        }
        self.slots[index].write.begin(generation);
        let Some(fence) = GlFence::record() else {
            return Err(PlayerError::Unavailable(
                "the OpenGL driver refused a fence sync object".to_string(),
            ));
        };
        let outcome = match fence.probe() {
            FenceOutcome::Signalled => FenceOutcome::Signalled,
            FenceOutcome::Waiting => fence.wait(FENCE_TIMEOUT),
            FenceOutcome::Failed => FenceOutcome::Failed,
        };
        if let Some(report_swap) = self.symbols.report_swap {
            unsafe { report_swap(self.context.as_ptr()) };
        }
        match outcome {
            FenceOutcome::Signalled => {}
            FenceOutcome::Waiting => {
                self.fence_timeouts += 1;
                self.skipped += 1;
                self.slots[index].fence = Some(fence);
                return Ok(None);
            }
            FenceOutcome::Failed => {
                return Err(PlayerError::Unavailable(
                    "the OpenGL fence for this frame could not be waited on".to_string(),
                ));
            }
        }
        self.slots[index].write.settle();
        self.slots[index].published_at = Some(Instant::now());
        if std::env::var_os("SCREENROOM_PLAYER_TRACE").is_some() {
            eprintln!(
                "[gl_render] {}x{} slot {} generation {generation} — libmpv {drawn:?}, fence waited",
                self.width, self.height, index
            );
        }
        let surface = Arc::clone(&self.slots[index].surface);
        Ok(Some(Frame::gpu(
            self.width,
            self.height,
            surface,
            generation,
        )))
    }

    fn settle_fences(&mut self) {
        for slot in &mut self.slots {
            let Some(fence) = slot.fence.as_ref() else {
                continue;
            };
            if fence.probe() == FenceOutcome::Signalled {
                slot.fence = None;
                slot.write.settle();
            }
        }
    }

    fn reap_surplus_slots(&mut self) {
        if self.slots.len() <= SURFACE_POOL_SIZE {
            return;
        }
        if self.slots.iter().any(|slot| {
            Arc::strong_count(&slot.surface) != 1 || slot.write.pending || slot.fence.is_some()
        }) {
            return;
        }
        while self.slots.len() > SURFACE_POOL_SIZE {
            let slot = self.slots.pop().expect("a slot was just counted");
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
    use crate::frame::TargetPrimaries;

    fn bare_slot() -> Slot {
        Slot {
            surface: Arc::new(SurfaceFrame::new(4, 4).expect("a surface exists")),
            texture: 0,
            framebuffer: 0,
            write: SlotWrite::default(),
            fence: None,
            published_at: None,
        }
    }

    fn pool(count: usize) -> Vec<Slot> {
        (0..count).map(|_| bare_slot()).collect()
    }

    fn published(slot: &mut Slot, generation: u64) {
        slot.write.begin(generation);
        slot.write.settle();
        slot.published_at = Some(Instant::now());
    }

    fn sampled(slot: &Slot, generation: u64) {
        slot.surface.reads().note_reading(generation);
    }

    fn release(slot: &Slot, generation: u64) {
        slot.surface.reads().note_released(generation);
    }

    #[test]
    fn a_free_slot_is_the_first_surface_nobody_holds() {
        let slots = pool(SURFACE_POOL_SIZE);
        let now = Instant::now();
        assert_eq!(writable_slot(&slots, now), Some(0));

        let held = Arc::clone(&slots[0].surface);
        assert_eq!(writable_slot(&slots, now), Some(1));
        drop(held);
        assert_eq!(writable_slot(&slots, now), Some(0));
    }

    #[test]
    fn a_pool_whose_surfaces_are_all_held_offers_nothing() {
        let slots = pool(SURFACE_POOL_SIZE);
        let now = Instant::now();
        let held: Vec<Arc<SurfaceFrame>> =
            slots.iter().map(|slot| Arc::clone(&slot.surface)).collect();
        assert_eq!(writable_slot(&slots, now), None);
        drop(held);
        assert_eq!(writable_slot(&slots, now), Some(0));
    }

    #[test]
    fn an_empty_pool_offers_nothing() {
        let slots: Vec<Slot> = Vec::new();
        assert_eq!(writable_slot(&slots, Instant::now()), None);
    }

    #[test]
    fn a_surface_is_not_handed_out_again_until_its_fence_has_signalled() {
        let mut slots = pool(1);
        let now = Instant::now();
        slots[0].write.begin(7);
        assert_eq!(
            writable_slot(&slots, now),
            None,
            "the GPU may still be drawing generation 7 into this surface"
        );
        slots[0].write.settle();
        assert_eq!(
            writable_slot(&slots, now),
            Some(0),
            "a signalled fence alone makes the slot usable again when nobody is reading it"
        );
    }

    #[test]
    fn a_surface_is_not_handed_out_again_until_the_consumer_says_it_is_done_with_it() {
        let mut slots = pool(1);
        let now = Instant::now();
        published(&mut slots[0], 9);
        sampled(&slots[0], 9);
        assert!(
            !slots[0].reads_reported(),
            "the consumer has not confirmed that generation 9 was read"
        );
        assert_eq!(
            writable_slot(&slots, now),
            None,
            "the picture on screen may still be sampled from this surface"
        );
        release(&slots[0], 9);
        assert!(slots[0].reads_reported());
        assert_eq!(writable_slot(&slots, now), Some(0));
    }

    #[test]
    fn a_surface_nobody_sampled_is_reusable_at_once() {
        let mut slots = pool(1);
        let now = Instant::now();
        published(&mut slots[0], 4);
        assert_eq!(
            writable_slot(&slots, now),
            Some(0),
            "a frame that was published but never imported has no reader to wait for, so a \
             hidden window cannot throttle the player"
        );
    }

    #[test]
    fn a_reader_that_never_reports_is_reclaimed_after_the_grace_period() {
        let mut slots = pool(1);
        let before = Instant::now();
        published(&mut slots[0], 3);
        sampled(&slots[0], 3);
        slots[0].published_at = Some(before);
        assert_eq!(writable_slot(&slots, before), None);
        assert_eq!(
            writable_slot(&slots, before + READER_GRACE - Duration::from_millis(1)),
            None,
            "the grace period must not be ridden through on its last millisecond"
        );
        assert_eq!(
            writable_slot(&slots, before + READER_GRACE),
            Some(0),
            "a lost completion report must not wedge the pool forever"
        );
        assert!(!slots[0].reads_reported());
    }

    #[test]
    fn a_pool_active_frames_still_being_read_skips_a_frame_instead_of_overwriting_one() {
        let mut slots = pool(SURFACE_POOL_SIZE);
        let now = Instant::now();
        for (index, slot) in slots.iter_mut().enumerate() {
            published(slot, index as u64 + 1);
            slot.surface.reads().note_reading(index as u64 + 1);
        }
        assert_eq!(
            writable_slot(&slots, now),
            None,
            "every surface is showing a frame, so the next frame is dropped rather than torn"
        );
        slots[1].surface.reads().note_released(2);
        assert_eq!(writable_slot(&slots, now), Some(1));
    }

    #[test]
    fn a_long_reuse_run_draws_every_generation_once_and_only_into_acknowledged_surfaces() {
        let mut slots = pool(SURFACE_POOL_SIZE);
        let now = Instant::now();
        let mut drawn = std::collections::BTreeSet::new();
        for generation in 1..=12_u64 {
            let index = writable_slot(&slots, now).expect("the consumer keeps releasing surfaces");
            assert!(
                slots[index].reads_reported(),
                "slot {index} was handed out while generation {} was still being read",
                slots[index].write.generation
            );
            assert!(
                drawn.insert(generation),
                "generation {generation} was drawn twice"
            );
            slots[index].write.begin(generation);
            slots[index].write.settle();
            slots[index].published_at = Some(now);
            sampled(&slots[index], generation);
            release(&slots[index], generation);
        }
        assert_eq!(drawn.len(), 12);
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

    #[test]
    fn the_eight_bit_gl_texture_is_the_bgra_format_the_shipped_path_uses() {
        assert_eq!(
            gl_formats(PixelFormat::Bgra8),
            (GL_RGBA8, GL_BGRA, GL_UNSIGNED_INT_8_8_8_8_REV)
        );
        assert_eq!(GL_RGBA8, 0x8058);
        assert_eq!(GL_BGRA, 0x80e1);
        assert_eq!(GL_UNSIGNED_INT_8_8_8_8_REV, 0x8367);
    }

    #[test]
    fn the_extended_texture_is_a_half_float_rgba_target() {
        assert_eq!(
            gl_formats(PixelFormat::Rgba16Float),
            (GL_RGBA16F, GL_RGBA, GL_HALF_FLOAT)
        );
        assert_eq!(GL_RGBA16F, 0x881a);
        assert_eq!(GL_RGBA, 0x1908);
        assert_eq!(GL_HALF_FLOAT, 0x140b);
    }

    #[test]
    fn extended_range_asks_mpv_to_tone_map_into_the_real_display() {
        let values = extended_range_values(DisplayTarget::with_headroom(2.0));
        let lookup = |key: &str| {
            values
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.as_str())
        };
        assert_eq!(lookup("target-prim"), Some("bt.709"));
        assert_eq!(lookup("target-trc"), Some("pq"));
        assert_eq!(lookup("target-peak"), Some("406"));
        assert_eq!(lookup("tone-mapping"), Some("bt.2390"));
        assert_eq!(lookup("hdr-compute-peak"), Some("no"));
    }

    #[test]
    fn an_assumed_display_gets_a_reference_white_tone_map_instead_of_a_guess() {
        let values = extended_range_values(DisplayTarget::sdr());
        let peak = values
            .iter()
            .find(|(name, _)| *name == "target-peak")
            .map(|(_, value)| value.as_str());
        assert_eq!(peak, Some("203"));
    }

    #[test]
    fn a_reset_restores_every_extended_range_option_to_auto() {
        let reset = extended_range_reset_values();
        for (name, value) in reset {
            assert_eq!(value, "auto", "{name} must return to auto");
        }
        assert_eq!(
            reset.len(),
            extended_range_values(DisplayTarget::sdr()).len()
        );
    }

    #[test]
    fn the_default_output_of_a_fresh_renderer_is_eight_bit_sdr() {
        assert_eq!(OutputDescription::default().format, PixelFormat::Bgra8);
        assert_eq!(
            OutputDescription::default().primaries,
            TargetPrimaries::Auto
        );
    }
}
