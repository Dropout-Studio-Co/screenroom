use std::ffi::{c_char, c_int, c_void};
use std::ptr::{self, NonNull};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::frame::PixelFormat;
use crate::surface::SurfaceFrame;
use crate::{Frame, LibMpv, Mpv, PlayerError, RenderSymbols, WakeSignal};

pub const UPDATE_FRAME: u64 = 1;

const SURFACE_POOL_SIZE: usize = 3;
const MAX_SURFACE_POOL_SIZE: usize = 16;

const FENCE_DEADLINE: Duration = Duration::from_secs(1);

const READER_GRACE: Duration = Duration::from_millis(1000);

const PENDING_PIPELINE_DEPTH: usize = 2;
const PENDING_MAX: usize = 3;
const TAIL_SLICE: Duration = Duration::from_millis(2);
const TAIL_DEADLINE: Duration = Duration::from_millis(50);

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
const GL_RGBA: u32 = 0x1908;
const GL_BGRA: u32 = 0x80e1;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
const GL_UNSIGNED_INT_8_8_8_8_REV: u32 = 0x8367;
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

enum Fence {
    Live(GlFence),
    #[cfg(test)]
    Stub(std::cell::Cell<FenceOutcome>),
}

impl Fence {
    fn record() -> Option<Self> {
        GlFence::record().map(Self::Live)
    }

    fn probe(&self) -> FenceOutcome {
        match self {
            Self::Live(fence) => fence.probe(),
            #[cfg(test)]
            Self::Stub(outcome) => outcome.get(),
        }
    }

    fn wait(&self, timeout: Duration) -> FenceOutcome {
        match self {
            Self::Live(fence) => fence.wait(timeout),
            #[cfg(test)]
            Self::Stub(outcome) => outcome.get(),
        }
    }

    #[cfg(test)]
    fn stub(outcome: FenceOutcome) -> Self {
        Self::Stub(std::cell::Cell::new(outcome))
    }

    #[cfg(test)]
    fn signal(&self) {
        if let Self::Stub(outcome) = self {
            outcome.set(FenceOutcome::Signalled);
        }
    }
}

impl std::fmt::Debug for Fence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Live(fence) => f.debug_tuple("Live").field(fence).finish(),
            #[cfg(test)]
            Self::Stub(outcome) => f.debug_tuple("Stub").field(&outcome.get()).finish(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingStep {
    Publish,
    Wait,
    Reclaim,
}

fn pending_step(probe: FenceOutcome, waited: Duration) -> PendingStep {
    match probe {
        FenceOutcome::Signalled => PendingStep::Publish,
        FenceOutcome::Failed => PendingStep::Reclaim,
        FenceOutcome::Waiting if waited >= FENCE_DEADLINE => PendingStep::Reclaim,
        FenceOutcome::Waiting => PendingStep::Wait,
    }
}

struct PendingFrame {
    generation: u64,
    slot: usize,
    fence: Fence,
    started: Instant,
}

impl std::fmt::Debug for PendingFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingFrame")
            .field("generation", &self.generation)
            .field("slot", &self.slot)
            .field("started", &self.started)
            .finish_non_exhaustive()
    }
}

struct PendingQueue {
    frames: Vec<PendingFrame>,
}

impl PendingQueue {
    fn new() -> Self {
        Self { frames: Vec::new() }
    }

    fn len(&self) -> usize {
        self.frames.len()
    }

    fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    fn front(&self) -> Option<&PendingFrame> {
        self.frames.first()
    }

    fn push(&mut self, frame: PendingFrame) -> bool {
        if self.frames.len() >= PENDING_MAX {
            return false;
        }
        self.frames.push(frame);
        true
    }

    fn pop_front(&mut self) -> Option<PendingFrame> {
        if self.frames.is_empty() {
            None
        } else {
            Some(self.frames.remove(0))
        }
    }

    fn holds_slot(&self, slot: usize) -> bool {
        self.frames.iter().any(|frame| frame.slot == slot)
    }

    fn clear(&mut self) -> Vec<PendingFrame> {
        std::mem::take(&mut self.frames)
    }
}

impl std::fmt::Debug for PendingQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(&self.frames).finish()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PublicationStats {
    pub generations_published: u64,
    pub generations_consumed: u64,
    pub pending_publishes: u64,
    pub publish_wait_max_ms: u64,
    pub fence_timeouts: u64,
    pub tail_waits: u64,
    pub skipped: u64,
    pub reclaimed_while_reading: u64,
    pub tear_samples: u64,
    pub tear_mismatches: u64,
    pub stale_repeats: u64,
}

static STATS: [AtomicU64; 11] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

pub fn publication_stats() -> PublicationStats {
    let read = |index: usize| STATS[index].load(Ordering::Acquire);
    PublicationStats {
        generations_published: read(0),
        generations_consumed: read(1),
        pending_publishes: read(2),
        publish_wait_max_ms: read(3),
        fence_timeouts: read(4),
        tail_waits: read(5),
        skipped: read(6),
        reclaimed_while_reading: read(7),
        tear_samples: read(8),
        tear_mismatches: read(9),
        stale_repeats: read(10),
    }
}

fn store_stats(stats: PublicationStats) {
    let values = [
        stats.generations_published,
        stats.generations_consumed,
        stats.pending_publishes,
        stats.publish_wait_max_ms,
        stats.fence_timeouts,
        stats.tail_waits,
        stats.skipped,
        stats.reclaimed_while_reading,
        stats.tear_samples,
        stats.tear_mismatches,
        stats.stale_repeats,
    ];
    for (slot, value) in STATS.iter().zip(values) {
        slot.store(value, Ordering::Release);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Counters {
    generations_published: u64,
    generations_consumed: u64,
    pending_publishes: u64,
    publish_wait_max_ms: u64,
    fence_timeouts: u64,
    tail_waits: u64,
    skipped: u64,
    reclaimed_while_reading: u64,
}

impl Counters {
    fn stats(self, tears: crate::surface::tearing::TearWatch) -> PublicationStats {
        PublicationStats {
            generations_published: self.generations_published,
            generations_consumed: self.generations_consumed,
            pending_publishes: self.pending_publishes,
            publish_wait_max_ms: self.publish_wait_max_ms,
            fence_timeouts: self.fence_timeouts,
            tail_waits: self.tail_waits,
            skipped: self.skipped,
            reclaimed_while_reading: self.reclaimed_while_reading,
            tear_samples: tears.samples(),
            tear_mismatches: tears.mismatches(),
            stale_repeats: tears.stale_repeats(),
        }
    }
}

fn reclaim_front(counters: &mut Counters, queue: &mut PendingQueue) -> Option<PendingFrame> {
    let pending = queue.pop_front()?;
    counters.fence_timeouts += 1;
    Some(pending)
}

fn wait_for_signalled(
    waited: Duration,
    deadline: Duration,
    slice: Duration,
    wait: &mut dyn FnMut(Duration) -> FenceOutcome,
    elapsed: &mut dyn FnMut() -> Duration,
) -> FenceOutcome {
    let mut waited = waited;
    loop {
        if waited >= deadline {
            return FenceOutcome::Waiting;
        }
        match wait(slice) {
            FenceOutcome::Waiting => waited = elapsed(),
            outcome => return outcome,
        }
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
    published_at: Option<Instant>,
    awaiting_release: bool,
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
            published_at: None,
            awaiting_release: false,
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
    pending: PendingQueue,
    counters: Counters,
    tears: crate::surface::tearing::TearWatch,
    tear_check: bool,
    output: PixelFormat,
    target_failure: Option<String>,
}

impl std::fmt::Debug for GlRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlRenderer")
            .field("size", &(self.width, self.height))
            .field("slots", &self.slots)
            .field("generation", &self.generation)
            .field("pending", &self.pending)
            .field("counters", &self.counters)
            .field("tear_check", &self.tear_check)
            .finish_non_exhaustive()
    }
}

pub fn clamp_target(width: u32, height: u32) -> (u32, u32) {
    (width.max(1), height.max(1))
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
            pending: PendingQueue::new(),
            counters: Counters::default(),
            tears: crate::surface::tearing::TearWatch::default(),
            tear_check: crate::surface::tearing::enabled(),
            output: PixelFormat::Bgra8,
            target_failure: None,
        })
    }

    pub fn take_dirty(&self) -> bool {
        self.callback_data.dirty.swap(false, Ordering::AcqRel)
    }

    pub fn update_flags(&self) -> Result<u64, PlayerError> {
        Ok(unsafe { (self.symbols.update)(self.context.as_ptr()) })
    }

    pub fn output(&self) -> PixelFormat {
        self.output
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
        self.abandon_pending();
        self.release_slots();
        let mut slots = Vec::with_capacity(SURFACE_POOL_SIZE);
        for _ in 0..SURFACE_POOL_SIZE {
            match Slot::create(&self.cgl, width, height, self.output) {
                Ok(slot) => slots.push(slot),
                Err(error) => {
                    self.target_failure = Some(error.to_string());
                    break;
                }
            }
        }
        self.slots = slots;
        self.publish_stats();
    }

    pub fn skipped(&self) -> u64 {
        self.counters.skipped
    }

    pub fn fence_timeouts(&self) -> u64 {
        self.counters.fence_timeouts
    }

    pub fn reclaimed_while_reading(&self) -> u64 {
        self.counters.reclaimed_while_reading
    }

    pub fn generations_published(&self) -> u64 {
        self.counters.generations_published
    }

    pub fn generations_consumed(&self) -> u64 {
        self.counters.generations_consumed
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn pipeline_depth(&self) -> usize {
        PENDING_PIPELINE_DEPTH
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    pub fn discard_pending(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        if self.cgl.make_current().is_err() {
            return;
        }
        for pending in self.pending.clear() {
            self.settle_slot(pending.slot);
            if let Some(report_swap) = self.symbols.report_swap {
                unsafe { report_swap(self.context.as_ptr()) };
            }
        }
        self.publish_stats();
    }

    pub fn publication_stats(&self) -> PublicationStats {
        self.counters.stats(self.tears)
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
        self.collect_consumed();
        let published = self.drain_pending()?;
        self.reap_surplus_slots();
        if self.pending.len() >= PENDING_MAX {
            self.counters.skipped += 1;
            self.publish_stats();
            return Ok(published);
        }
        let index = match writable_slot(&self.slots, Instant::now()) {
            Some(index) => index,
            None if self.slots.len() < MAX_SURFACE_POOL_SIZE => {
                match Slot::create(&self.cgl, self.width, self.height, self.output) {
                    Ok(slot) => {
                        self.slots.push(slot);
                        self.slots.len() - 1
                    }
                    Err(_) => {
                        self.counters.skipped += 1;
                        self.publish_stats();
                        return Ok(published);
                    }
                }
            }
            None => {
                self.counters.skipped += 1;
                self.publish_stats();
                return Ok(published);
            }
        };
        if !self.slots[index].reads_reported() {
            self.counters.reclaimed_while_reading += 1;
        }
        let framebuffer = self.slots[index].framebuffer;
        let (internal_format, _, _) = gl_formats(self.output);
        let mut fbo = OpenGlFbo {
            fbo: framebuffer as c_int,
            w: self.width as c_int,
            h: self.height as c_int,
            internal_format: internal_format as c_int,
        };
        let mut flip_y: c_int = 0;
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
        let Some(fence) = Fence::record() else {
            return Err(PlayerError::Unavailable(
                "the OpenGL driver refused a fence sync object".to_string(),
            ));
        };
        let queued = PendingFrame {
            generation,
            slot: index,
            fence,
            started: Instant::now(),
        };
        if !self.pending.push(queued) {
            self.slots[index].write.settle();
            self.counters.skipped += 1;
            self.publish_stats();
            return Ok(published);
        }
        if std::env::var_os("SCREENROOM_PLAYER_TRACE").is_some() {
            eprintln!(
                "[gl_render] {}x{} slot {} generation {generation} — libmpv {drawn:?}, fence recorded, {} pending",
                self.width,
                self.height,
                index,
                self.pending.len()
            );
        }
        self.publish_stats();
        Ok(published)
    }

    pub fn publish_pending(&mut self) -> Result<Option<Frame>, PlayerError> {
        if self.pending.is_empty() {
            return Ok(None);
        }
        if let Some(reason) = &self.target_failure {
            return Err(PlayerError::Unavailable(format!(
                "the OpenGL surface pool is not ready: {reason}"
            )));
        }
        self.cgl.make_current()?;
        self.counters.tail_waits += 1;
        let began = Instant::now();
        while let Some(front) = self.pending.front() {
            let step = pending_step(front.fence.probe(), front.started.elapsed());
            match step {
                PendingStep::Publish => {
                    let frame = self.publish_front(Instant::now(), false)?;
                    self.collect_consumed();
                    self.publish_stats();
                    return Ok(Some(frame));
                }
                PendingStep::Reclaim => {
                    if let Some(frame) = reclaim_front(&mut self.counters, &mut self.pending) {
                        self.settle_slot(frame.slot);
                    }
                }
                PendingStep::Wait => {
                    let outcome = wait_for_signalled(
                        Duration::ZERO,
                        TAIL_DEADLINE,
                        TAIL_SLICE,
                        &mut |slice| {
                            self.pending
                                .front()
                                .map(|pending| pending.fence.wait(slice))
                                .unwrap_or(FenceOutcome::Signalled)
                        },
                        &mut || began.elapsed(),
                    );
                    match outcome {
                        FenceOutcome::Signalled => {}
                        FenceOutcome::Waiting => break,
                        FenceOutcome::Failed => {
                            return Err(PlayerError::Unavailable(
                                "the OpenGL fence for this frame could not be waited on"
                                    .to_string(),
                            ));
                        }
                    }
                }
            }
        }
        self.collect_consumed();
        self.publish_stats();
        Ok(None)
    }

    fn drain_pending(&mut self) -> Result<Option<Frame>, PlayerError> {
        let mut published = None;
        while let Some(front) = self.pending.front() {
            let step = pending_step(front.fence.probe(), front.started.elapsed());
            match step {
                PendingStep::Publish => {
                    if published.is_some() {
                        break;
                    }
                    published = Some(self.publish_front(Instant::now(), true)?);
                }
                PendingStep::Reclaim => {
                    if let Some(frame) = reclaim_front(&mut self.counters, &mut self.pending) {
                        self.settle_slot(frame.slot);
                    }
                }
                PendingStep::Wait => break,
            }
        }
        Ok(published)
    }

    fn publish_front(&mut self, now: Instant, drained: bool) -> Result<Frame, PlayerError> {
        let Some(pending) = self.pending.pop_front() else {
            return Err(PlayerError::Unavailable(
                "a frame was published from an empty queue".to_string(),
            ));
        };
        let waited = now.saturating_duration_since(pending.started);
        self.counters.publish_wait_max_ms = self
            .counters
            .publish_wait_max_ms
            .max(waited.as_millis() as u64);
        if drained {
            self.counters.pending_publishes += 1;
        }
        if let Some(report_swap) = self.symbols.report_swap {
            unsafe { report_swap(self.context.as_ptr()) };
        }
        let slot = &mut self.slots[pending.slot];
        slot.write.settle();
        slot.published_at = Some(now);
        slot.awaiting_release = true;
        if self.tear_check
            && let Some(strips) = crate::surface::tearing::sample_strips(&slot.surface)
        {
            self.tears.observe(strips);
        }
        let surface = Arc::clone(&slot.surface);
        self.counters.generations_published += 1;
        Ok(Frame::gpu(
            self.width,
            self.height,
            surface,
            pending.generation,
        ))
    }

    fn settle_slot(&mut self, index: usize) {
        if let Some(slot) = self.slots.get_mut(index) {
            slot.write.settle();
        }
    }

    fn collect_consumed(&mut self) {
        for slot in &mut self.slots {
            if slot.awaiting_release && slot.reads_reported() {
                slot.awaiting_release = false;
                self.counters.generations_consumed += 1;
            }
        }
    }

    fn publish_stats(&self) {
        store_stats(self.counters.stats(self.tears));
    }

    fn reap_surplus_slots(&mut self) {
        if self.slots.len() <= SURFACE_POOL_SIZE {
            return;
        }
        let busy = self.slots.iter().enumerate().any(|(index, slot)| {
            Arc::strong_count(&slot.surface) != 1
                || slot.write.pending
                || self.pending.holds_slot(index)
                || !slot.reads_reported()
        });
        if busy {
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

    fn abandon_pending(&mut self) {
        let abandoned = self.pending.clear();
        for pending in abandoned {
            if let Some(slot) = self.slots.get_mut(pending.slot) {
                slot.write.settle();
            }
        }
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
            self.abandon_pending();
            self.release_slots();
        }
        self.callback_data.dirty.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bare_slot() -> Slot {
        Slot {
            surface: Arc::new(SurfaceFrame::new(4, 4).expect("a surface exists")),
            texture: 0,
            framebuffer: 0,
            write: SlotWrite::default(),
            published_at: None,
            awaiting_release: false,
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

    fn pending(generation: u64, slot: usize, outcome: FenceOutcome) -> PendingFrame {
        PendingFrame {
            generation,
            slot,
            fence: Fence::stub(outcome),
            started: Instant::now(),
        }
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
    fn a_pending_frame_is_not_published_before_its_fence_signals() {
        let frame = pending(1, 0, FenceOutcome::Waiting);
        assert_eq!(
            pending_step(frame.fence.probe(), Duration::from_millis(1)),
            PendingStep::Wait,
            "a frame whose fence has not signalled may still be half drawn"
        );
        frame.fence.signal();
        assert_eq!(
            pending_step(frame.fence.probe(), Duration::from_millis(1)),
            PendingStep::Publish
        );
    }

    #[test]
    fn frames_are_published_in_the_order_they_were_drawn_and_each_exactly_once() {
        let mut queue = PendingQueue::new();
        for generation in 1..=PENDING_MAX as u64 {
            assert!(queue.push(pending(
                generation,
                generation as usize - 1,
                FenceOutcome::Waiting
            )));
        }
        let mut published = Vec::new();
        while let Some(frame) = queue.pop_front() {
            frame.fence.signal();
            published.push(frame.generation);
        }
        assert_eq!(published, vec![1, 2, 3]);
        assert!(queue.is_empty());
        assert_eq!(
            published
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            published.len(),
            "no generation may be published twice"
        );
    }

    #[test]
    fn a_full_queue_skips_a_frame_instead_of_overwriting_one_in_flight() {
        let mut queue = PendingQueue::new();
        for generation in 1..=PENDING_MAX as u64 {
            assert!(queue.push(pending(generation, 0, FenceOutcome::Waiting)));
        }
        let full = pending(99, 1, FenceOutcome::Waiting);
        assert!(
            !queue.push(full),
            "the queue is bounded, so a full one refuses rather than recycles a live fence"
        );
        assert_eq!(queue.len(), PENDING_MAX);
        assert_eq!(
            queue.front().map(|frame| frame.generation),
            Some(1),
            "the oldest frame keeps its place"
        );
        assert_eq!(PENDING_MAX, 3);
        assert_eq!(PENDING_PIPELINE_DEPTH, 2);
    }

    #[test]
    fn a_lost_fence_is_reclaimed_after_the_deadline_and_counted_apart_from_skips() {
        let mut counters = Counters::default();
        let mut queue = PendingQueue::new();
        queue.push(pending(5, 2, FenceOutcome::Waiting));
        let front = queue.front().expect("the frame is queued");
        assert_eq!(
            pending_step(
                front.fence.probe(),
                FENCE_DEADLINE - Duration::from_millis(1)
            ),
            PendingStep::Wait
        );
        assert_eq!(
            pending_step(front.fence.probe(), FENCE_DEADLINE),
            PendingStep::Reclaim,
            "a fence that never reports must eventually be given up on"
        );
        let reclaimed = reclaim_front(&mut counters, &mut queue).expect("the frame was reclaimed");
        assert_eq!(reclaimed.generation, 5);
        assert_eq!(counters.fence_timeouts, 1);
        assert_eq!(counters.skipped, 0);
        assert!(queue.is_empty());
    }

    #[test]
    fn the_tail_poll_publishes_the_last_frame_of_a_pause() {
        let clock = std::cell::Cell::new(Duration::ZERO);
        let mut outcomes = std::collections::VecDeque::from([
            FenceOutcome::Waiting,
            FenceOutcome::Waiting,
            FenceOutcome::Signalled,
        ]);
        let outcome = wait_for_signalled(
            Duration::ZERO,
            TAIL_DEADLINE,
            TAIL_SLICE,
            &mut |slice| {
                clock.set(clock.get() + slice);
                outcomes.pop_front().unwrap_or(FenceOutcome::Signalled)
            },
            &mut || clock.get(),
        );
        assert_eq!(
            outcome,
            FenceOutcome::Signalled,
            "the frame drawn just before a pause must still reach the screen"
        );
        assert!(clock.get() <= TAIL_DEADLINE);
    }

    #[test]
    fn the_tail_poll_gives_up_rather_than_stalling_the_worker() {
        let clock = std::cell::Cell::new(Duration::ZERO);
        let outcome = wait_for_signalled(
            Duration::ZERO,
            TAIL_DEADLINE,
            TAIL_SLICE,
            &mut |slice| {
                clock.set(clock.get() + slice);
                FenceOutcome::Waiting
            },
            &mut || clock.get(),
        );
        assert_eq!(outcome, FenceOutcome::Waiting);
        let waited = clock.get();
        assert!(
            waited <= TAIL_DEADLINE,
            "the tail wait is bounded by its deadline, not by the fence: {waited:?}"
        );
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
}
