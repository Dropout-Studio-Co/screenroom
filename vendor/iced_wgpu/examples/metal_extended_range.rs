//! Reads the extended range state of a real `CAMetalLayer` without opening a window.
#![allow(unsafe_code)]

use core::ffi::c_void;
use core::ptr::NonNull;

use objc2::MainThreadMarker;
use objc2_app_kit::NSView;
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, DisplayHandle, HandleError, HasDisplayHandle,
    HasWindowHandle, WindowHandle,
};

const WIDTH: f64 = 32.0;
const HEIGHT: f64 = 18.0;

#[derive(Clone)]
struct TestView(NonNull<NSView>);

unsafe impl Send for TestView {}

unsafe impl Sync for TestView {}

impl HasWindowHandle for TestView {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let handle = AppKitWindowHandle::new(self.0.cast::<c_void>());
        Ok(unsafe { WindowHandle::borrow_raw(handle.into()) })
    }
}

impl HasDisplayHandle for TestView {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let handle = AppKitDisplayHandle::new();
        Ok(unsafe { DisplayHandle::borrow_raw(handle.into()) })
    }
}

fn main() {
    let Some(marker) = MainThreadMarker::new() else {
        eprintln!("this receipt must run on the main thread");
        std::process::exit(1);
    };

    let device = Device::open();

    println!("[hardware] metal_device {}", adapter_name(&device));
    println!(
        "[capability] potential_edr_headroom {}",
        potential_headroom(marker)
    );

    for (label, format) in [
        ("sdr", wgpu::TextureFormat::Bgra8UnormSrgb),
        ("extended", wgpu::TextureFormat::Rgba16Float),
    ] {
        let view = NSView::initWithFrame(
            marker.alloc(),
            CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(WIDTH, HEIGHT)),
        );
        view.setWantsLayer(true);
        let pointer = NonNull::from(&*view);
        let view_pointer = pointer.cast::<c_void>();

        eprintln!("[{label}] creating surface");
        let surface = device
            .instance
            .create_surface(TestView(pointer))
            .expect("the view accepts a surface");
        eprintln!("[{label}] configuring");
        surface.configure(
            &device.device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format,
                present_mode: wgpu::PresentMode::Fifo,
                width: WIDTH as u32,
                height: HEIGHT as u32,
                alpha_mode: wgpu::CompositeAlphaMode::Opaque,
                view_formats: vec![],
                desired_maximum_frame_latency: 1,
            },
        );

        let before = iced_wgpu::window::compositor::extended_range_layer_state(view_pointer)
            .expect("the layer reports its state");

        if format == wgpu::TextureFormat::Rgba16Float {
            unsafe {
                iced_wgpu::window::compositor::apply_extended_range_colorspace(view_pointer);
            };
        }

        let after = iced_wgpu::window::compositor::extended_range_layer_state(view_pointer)
            .expect("the layer reports its state");

        println!("[{label}] swapchain_format {format:?}");
        println!("[{label}] layer_pixel_format {}", before.pixel_format);
        println!(
            "[{label}] layer_wants_extended_dynamic_range {}",
            before.wants_extended_range
        );
        println!(
            "[{label}] layer_colorspace_wgpu {}",
            before
                .colorspace
                .clone()
                .unwrap_or_else(|| "nil".to_string())
        );
        println!(
            "[{label}] layer_colorspace_fork {}",
            after
                .colorspace
                .clone()
                .unwrap_or_else(|| "nil".to_string())
        );

        assert_eq!(
            before.pixel_format,
            metal_pixel_format(format),
            "the layer must present the swapchain format"
        );
        assert_eq!(
            before.wants_extended_range,
            format == wgpu::TextureFormat::Rgba16Float,
            "only the half float swapchain asks the window server for extended range"
        );

        if format == wgpu::TextureFormat::Rgba16Float {
            assert_eq!(
                after.colorspace.as_deref(),
                Some("kCGColorSpaceExtendedSRGB"),
                "the fork must set an extended linear colour space"
            );
        } else {
            assert_eq!(
                after.colorspace.as_deref(),
                Some("kCGColorSpaceSRGB"),
                "the eight bit control keeps the layer's own sRGB colour space"
            );
            assert!(
                !after.wants_extended_range,
                "the eight bit control must not ask for extended range"
            );
        }

        use std::io::Write;
        let _ = std::io::stdout().flush();

        drop(surface);
        let settled = device
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the device settles between configurations");
        let _ = settled;
    }

    println!("[receipt] ok");
}

fn metal_pixel_format(format: wgpu::TextureFormat) -> u64 {
    match format {
        wgpu::TextureFormat::Bgra8UnormSrgb => 81,
        wgpu::TextureFormat::Rgba16Float => 115,
        other => panic!("unexpected format {other:?}"),
    }
}

fn adapter_name(device: &Device) -> String {
    device.adapter.get_info().name
}

fn potential_headroom(marker: MainThreadMarker) -> f64 {
    objc2_app_kit::NSScreen::mainScreen(marker)
        .map(|screen| screen.maximumPotentialExtendedDynamicRangeColorComponentValue())
        .unwrap_or(1.0)
}

struct Device {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    _queue: wgpu::Queue,
}

impl Device {
    fn open() -> Self {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("a Metal adapter exists");
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .expect("a Metal device opens");
        Self {
            instance,
            adapter,
            device,
            _queue: queue,
        }
    }
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};

    struct ThreadWaker(std::thread::Thread);

    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}
