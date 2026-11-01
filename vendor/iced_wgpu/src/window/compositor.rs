//! Connect a window with a renderer.
use crate::core::Color;
use crate::graphics::color;
use crate::graphics::compositor;
use crate::graphics::error;
use crate::graphics::{self, Shell, Viewport};
use crate::settings::{self, Settings};
use crate::{Engine, Renderer};

/// The environment variable that asks for an extended range swapchain.
///
/// Screenroom vendors this crate to select `Rgba16Float` and the extended linear
/// sRGB colour space on the window layer, which is what makes Metal present the
/// frame with extended dynamic range. It is the same switch that makes the player
/// deliver half-float frames, so the two can never disagree. Absent or `sdr`
/// leaves every existing code path untouched.
pub const EXTENDED_RANGE_ENVIRONMENT: &str = "SCREENROOM_HDR";

/// The swapchain format that carries extended dynamic range.
pub const EXTENDED_RANGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Returns whether the environment asks for the extended range swapchain.
pub fn extended_range_requested() -> bool {
    match std::env::var(EXTENDED_RANGE_ENVIRONMENT) {
        Ok(value) => value.trim().eq_ignore_ascii_case("extended"),
        Err(_) => false,
    }
}

/// A window graphics backend for iced powered by `wgpu`.
pub struct Compositor {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    format: wgpu::TextureFormat,
    alpha_mode: wgpu::CompositeAlphaMode,
    engine: Engine,
    settings: Settings,
}

/// A compositor error.
#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    /// The surface creation failed.
    #[error("the surface creation failed: {0}")]
    SurfaceCreationFailed(#[from] wgpu::CreateSurfaceError),
    /// The surface is not compatible.
    #[error("the surface is not compatible")]
    IncompatibleSurface,
    /// No adapter was found for the options requested.
    #[error("no adapter was found for the options requested: {0:?}")]
    NoAdapterFound(String),
    /// No device request succeeded.
    #[error("no device request succeeded: {0:?}")]
    RequestDeviceFailed(Vec<(wgpu::Limits, wgpu::RequestDeviceError)>),
}

impl From<Error> for graphics::Error {
    fn from(error: Error) -> Self {
        Self::GraphicsAdapterNotFound {
            backend: "wgpu",
            reason: error::Reason::RequestFailed(error.to_string()),
        }
    }
}

impl Compositor {
    /// Requests a new [`Compositor`] with the given [`Settings`].
    ///
    /// Returns `None` if no compatible graphics adapter could be found.
    pub async fn request<W: compositor::Window>(
        settings: Settings,
        compatible_window: Option<W>,
        shell: Shell,
    ) -> Result<Self, Error> {
        let instance = wgpu::util::new_instance_with_webgpu_detection(&wgpu::InstanceDescriptor {
            backends: settings.backends,
            flags: if cfg!(feature = "strict-assertions") {
                wgpu::InstanceFlags::debugging()
            } else {
                wgpu::InstanceFlags::empty()
            },
            ..Default::default()
        })
        .await;

        log::info!("{settings:#?}");

        #[cfg(not(target_arch = "wasm32"))]
        if log::max_level() >= log::LevelFilter::Info {
            let available_adapters: Vec<_> = instance
                .enumerate_adapters(settings.backends)
                .iter()
                .map(wgpu::Adapter::get_info)
                .collect();
            log::info!("Available adapters: {available_adapters:#?}");
        }

        #[allow(unsafe_code)]
        let compatible_surface =
            compatible_window.and_then(|window| instance.create_surface(window).ok());

        let adapter_options = wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::from_env()
                .unwrap_or(wgpu::PowerPreference::HighPerformance),
            compatible_surface: compatible_surface.as_ref(),
            force_fallback_adapter: false,
        };

        let adapter = instance
            .request_adapter(&adapter_options)
            .await
            .map_err(|_error| Error::NoAdapterFound(format!("{adapter_options:?}")))?;

        log::info!("Selected: {:#?}", adapter.get_info());

        let (format, alpha_mode) = compatible_surface
            .as_ref()
            .and_then(|surface| {
                let capabilities = surface.get_capabilities(&adapter);

                let formats = capabilities.formats.iter().copied();

                log::info!("Available formats: {formats:#?}");

                let usable: Vec<_> = capabilities
                    .formats
                    .iter()
                    .copied()
                    .filter(|format| {
                        format.required_features() == wgpu::Features::empty()
                    })
                    .collect();

                let extended_range = extended_range_requested();

                if extended_range
                    && !capabilities.formats.contains(&EXTENDED_RANGE_FORMAT)
                {
                    log::warn!(
                        "extended range was requested but this surface does not offer {EXTENDED_RANGE_FORMAT:?}"
                    );
                }

                let format = select_format(
                    &usable,
                    extended_range,
                    color::GAMMA_CORRECTION,
                );

                let format = format.or_else(|| {
                    log::warn!("No format found!");

                    capabilities.formats.first().copied()
                });

                let alpha_modes = capabilities.alpha_modes;

                log::info!("Available alpha modes: {alpha_modes:#?}");

                let preferred_alpha = if alpha_modes
                    .contains(&wgpu::CompositeAlphaMode::PostMultiplied)
                {
                    wgpu::CompositeAlphaMode::PostMultiplied
                } else if alpha_modes
                    .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
                {
                    wgpu::CompositeAlphaMode::PreMultiplied
                } else {
                    wgpu::CompositeAlphaMode::Auto
                };

                format.zip(Some(preferred_alpha))
            })
            .ok_or(Error::IncompatibleSurface)?;

        log::info!("Selected format: {format:?} with alpha mode: {alpha_mode:?}");

        #[cfg(target_arch = "wasm32")]
        let limits = [wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits())];

        #[cfg(not(target_arch = "wasm32"))]
        let limits = [wgpu::Limits::default(), wgpu::Limits::downlevel_defaults()];

        let limits = limits.into_iter().map(|limits| wgpu::Limits {
            max_bind_groups: 2,
            max_non_sampler_bindings: 2048,
            ..limits
        });

        let mut errors = Vec::new();

        for required_limits in limits {
            let result = adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("iced_wgpu::window::compositor device descriptor"),
                    required_features: wgpu::Features::empty(),
                    required_limits: required_limits.clone(),
                    memory_hints: wgpu::MemoryHints::MemoryUsage,
                    trace: wgpu::Trace::Off,
                    experimental_features: wgpu::ExperimentalFeatures::disabled(),
                })
                .await;

            match result {
                Ok((device, queue)) => {
                    let engine = Engine::new(
                        &adapter,
                        device,
                        queue,
                        format,
                        settings.antialiasing,
                        shell,
                    );

                    return Ok(Compositor {
                        instance,
                        adapter,
                        format,
                        alpha_mode,
                        engine,
                        settings,
                    });
                }
                Err(error) => {
                    errors.push((required_limits, error));
                }
            }
        }

        Err(Error::RequestDeviceFailed(errors))
    }
}

/// Creates a [`Compositor`] with the given [`Settings`] and window.
pub async fn new<W: compositor::Window>(
    settings: Settings,
    compatible_window: W,
    shell: Shell,
) -> Result<Compositor, Error> {
    Compositor::request(settings, Some(compatible_window), shell).await
}

/// Presents the given primitives with the given [`Compositor`].
pub fn present(
    renderer: &mut Renderer,
    surface: &mut wgpu::Surface<'static>,
    viewport: &Viewport,
    background_color: Color,
    on_pre_present: impl FnOnce(),
) -> Result<(), compositor::SurfaceError> {
    match surface.get_current_texture() {
        Ok(frame) => {
            let view = &frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());

            let _submission = renderer.present(
                Some(background_color),
                frame.texture.format(),
                view,
                viewport,
            );

            // Present the frame
            on_pre_present();
            frame.present();

            Ok(())
        }
        Err(error) => match error {
            wgpu::SurfaceError::Timeout => Err(compositor::SurfaceError::Timeout),
            wgpu::SurfaceError::Outdated => Err(compositor::SurfaceError::Outdated),
            wgpu::SurfaceError::Lost => Err(compositor::SurfaceError::Lost),
            wgpu::SurfaceError::OutOfMemory => Err(compositor::SurfaceError::OutOfMemory),
            wgpu::SurfaceError::Other => Err(compositor::SurfaceError::Other),
        },
    }
}

impl graphics::Compositor for Compositor {
    type Renderer = Renderer;
    type Surface = wgpu::Surface<'static>;

    async fn with_backend(
        settings: graphics::Settings,
        _display: impl compositor::Display,
        compatible_window: impl compositor::Window,
        shell: Shell,
        backend: Option<&str>,
    ) -> Result<Self, graphics::Error> {
        match backend {
            None | Some("wgpu") => {
                let mut settings = Settings::from(settings);

                if let Some(backends) = wgpu::Backends::from_env() {
                    settings.backends = backends;
                }

                if let Some(present_mode) = settings::present_mode_from_env() {
                    settings.present_mode = present_mode;
                }

                Ok(new(settings, compatible_window, shell).await?)
            }
            Some(backend) => Err(graphics::Error::GraphicsAdapterNotFound {
                backend: "wgpu",
                reason: error::Reason::DidNotMatch {
                    preferred_backend: backend.to_owned(),
                },
            }),
        }
    }

    fn create_renderer(&self) -> Self::Renderer {
        Renderer::new(
            self.engine.clone(),
            self.settings.default_font,
            self.settings.default_text_size,
        )
    }

    #[allow(unsafe_code)]
    fn create_surface<W: compositor::Window>(
        &mut self,
        window: W,
        width: u32,
        height: u32,
    ) -> Self::Surface {
        #[cfg(target_os = "macos")]
        let extended_range_view = if self.format == EXTENDED_RANGE_FORMAT {
            extended_range::ns_view(&window)
        } else {
            None
        };

        let mut surface = self
            .instance
            .create_surface(window)
            .expect("Create surface");

        if width > 0 && height > 0 {
            self.configure_surface(&mut surface, width, height);
        }

        #[cfg(target_os = "macos")]
        if let Some(view) = extended_range_view {
            unsafe { extended_range::apply_colorspace(view) };
        }

        surface
    }

    fn configure_surface(&mut self, surface: &mut Self::Surface, width: u32, height: u32) {
        surface.configure(
            &self.engine.device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: self.format,
                present_mode: self.settings.present_mode,
                width,
                height,
                alpha_mode: self.alpha_mode,
                view_formats: vec![],
                desired_maximum_frame_latency: 1,
            },
        );
    }

    fn information(&self) -> compositor::Information {
        let information = self.adapter.get_info();

        compositor::Information {
            adapter: information.name,
            backend: format!("{:?}", information.backend),
        }
    }

    fn present(
        &mut self,
        renderer: &mut Self::Renderer,
        surface: &mut Self::Surface,
        viewport: &Viewport,
        background_color: Color,
        on_pre_present: impl FnOnce(),
    ) -> Result<(), compositor::SurfaceError> {
        present(
            renderer,
            surface,
            viewport,
            background_color,
            on_pre_present,
        )
    }

    fn screenshot(
        &mut self,
        renderer: &mut Self::Renderer,
        viewport: &Viewport,
        background_color: Color,
    ) -> Vec<u8> {
        renderer.screenshot(viewport, background_color)
    }
}

/// The extended range state of a window's Metal layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerState {
    /// The `MTLPixelFormat` the layer presents.
    pub pixel_format: u64,

    /// Whether the layer asked the window server for extended range content.
    pub wants_extended_range: bool,

    /// The CoreGraphics colour space name set on the layer, if any.
    pub colorspace: Option<String>,
}

#[cfg(target_os = "macos")]
pub use extended_range::{
    apply_colorspace as apply_extended_range_colorspace, layer_state as extended_range_layer_state,
};

/// Chooses the swapchain format from the formats a surface offers.
///
/// This is the whole of this fork's format behaviour: `Rgba16Float` when the
/// extended range was asked for and the surface offers it, otherwise exactly the
/// selection that upstream iced makes, so a request that cannot be honoured and a
/// request that was never made both leave the eight-bit path untouched.
pub fn select_format(
    available: &[wgpu::TextureFormat],
    extended_range: bool,
    gamma_correction: bool,
) -> Option<wgpu::TextureFormat> {
    if extended_range && available.contains(&EXTENDED_RANGE_FORMAT) {
        return Some(EXTENDED_RANGE_FORMAT);
    }

    let mut available = available.iter().copied();

    if gamma_correction {
        available.find(wgpu::TextureFormat::is_srgb)
    } else {
        available.find(|format| !wgpu::TextureFormat::is_srgb(format))
    }
}

#[cfg(target_os = "macos")]
mod extended_range {
    use core::ffi::c_void;
    use core::ptr::NonNull;
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2_core_foundation::CFRetained;
    use objc2_core_graphics::{CGColorSpace, kCGColorSpaceExtendedSRGB};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    #[allow(unsafe_code)]
    #[link(name = "QuartzCore", kind = "framework")]
    unsafe extern "C" {}

    const MAXIMUM_LAYER_DEPTH: usize = 4;

    pub(super) fn ns_view(window: &impl HasWindowHandle) -> Option<NonNull<c_void>> {
        match window.window_handle().ok()?.as_raw() {
            RawWindowHandle::AppKit(handle) => Some(handle.ns_view),
            _ => None,
        }
    }

    #[allow(unsafe_code)]
    /// Sets the extended linear sRGB colour space on the window's Metal layer.
    pub unsafe fn apply_colorspace(view: NonNull<c_void>) {
        let layer = metal_layer_of(view);
        let Some(layer) = layer else {
            log::warn!("extended range is active but no CAMetalLayer was found on the window");
            return;
        };

        let named = unsafe { kCGColorSpaceExtendedSRGB };

        let Some(space) = CGColorSpace::with_name(Some(named)) else {
            log::warn!("the extended linear sRGB colour space is unavailable");
            return;
        };

        let space = CFRetained::as_ptr(&space)
            .as_ptr()
            .cast::<objc2_core_graphics::CGColorSpace>();

        let _: () = unsafe { msg_send![layer, setColorspace: space] };

        log::info!("extended range colours space set on the window layer");
    }

    #[allow(unsafe_code)]
    pub(super) fn metal_layer_of(view: NonNull<c_void>) -> Option<*mut AnyObject> {
        let view: *mut AnyObject = view.as_ptr().cast();

        let root: *mut AnyObject = unsafe { msg_send![view, layer] };
        if root.is_null() {
            return None;
        }

        unsafe { metal_layer(root, 0) }
    }

    #[allow(unsafe_code)]
    /// Reads the extended range state of the window's Metal layer.
    pub fn layer_state(view: NonNull<c_void>) -> Option<super::LayerState> {
        let layer = metal_layer_of(view)?;

        let pixel_format: u64 = unsafe { msg_send![layer, pixelFormat] };
        let wants: Bool = unsafe { msg_send![layer, wantsExtendedDynamicRangeContent] };

        let space: *mut objc2_core_graphics::CGColorSpace = unsafe { msg_send![layer, colorspace] };
        let colorspace = match unsafe { space.as_ref() } {
            Some(space) => CGColorSpace::get_name(Some(space)).map(|name| name.to_string()),
            None => None,
        };

        Some(super::LayerState {
            pixel_format,
            wants_extended_range: wants.as_bool(),
            colorspace,
        })
    }

    #[allow(unsafe_code)]
    unsafe fn metal_layer(layer: *mut AnyObject, depth: usize) -> Option<*mut AnyObject> {
        if layer.is_null() || depth > MAXIMUM_LAYER_DEPTH {
            return None;
        }

        if let Some(metal_layer_class) = AnyClass::get(c"CAMetalLayer") {
            let is_metal: Bool = unsafe { msg_send![layer, isKindOfClass: metal_layer_class] };
            if is_metal.as_bool() {
                return Some(layer);
            }
        }

        let sublayers: *mut AnyObject = unsafe { msg_send![layer, sublayers] };
        if sublayers.is_null() {
            return None;
        }

        let count: usize = unsafe { msg_send![sublayers, count] };
        for index in 0..count {
            let child: *mut AnyObject = unsafe { msg_send![sublayers, objectAtIndex: index] };
            if let Some(found) = unsafe { metal_layer(child, depth + 1) } {
                return Some(found);
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SDR: &[wgpu::TextureFormat] = &[
        wgpu::TextureFormat::Bgra8Unorm,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba16Float,
    ];

    #[test]
    fn a_request_that_was_not_made_keeps_the_upstream_srgb_choice() {
        assert_eq!(
            select_format(SDR, false, true),
            Some(wgpu::TextureFormat::Bgra8UnormSrgb)
        );
    }

    #[test]
    fn an_extended_range_request_selects_the_half_float_surface() {
        assert_eq!(
            select_format(SDR, true, true),
            Some(wgpu::TextureFormat::Rgba16Float)
        );
    }

    #[test]
    fn a_surface_without_half_float_falls_back_to_the_srgb_choice() {
        let eight_bit_only = [
            wgpu::TextureFormat::Bgra8UnormSrgb,
            wgpu::TextureFormat::Bgra8Unorm,
        ];
        assert_eq!(
            select_format(&eight_bit_only, true, true),
            Some(wgpu::TextureFormat::Bgra8UnormSrgb)
        );
    }

    #[test]
    fn a_surface_with_only_linear_formats_leaves_the_fallback_to_the_caller() {
        let linear_only = [wgpu::TextureFormat::Bgra8Unorm];
        assert_eq!(select_format(&linear_only, false, true), None);
    }

    #[test]
    fn an_absent_switch_is_not_an_extended_range_request() {
        assert!(EXTENDED_RANGE_ENVIRONMENT.contains("HDR"));
        assert_eq!(EXTENDED_RANGE_FORMAT, wgpu::TextureFormat::Rgba16Float);
    }
}
