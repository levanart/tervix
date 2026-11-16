//! Surface-free WGPU initialization for real widget-tree and Canvas tests.
use iced_wgpu::{Engine, wgpu};
use std::sync::{Mutex, OnceLock};

#[allow(clippy::expect_used)] // Test fixture initialization failures must fail the test.
pub(super) fn renderer(font: iced::Font) -> iced::Renderer {
    // Share device/pipelines across fixtures; no window, surface or event loop is
    // created. An unavailable adapter is a test failure, never a silent skip.
    static CONTEXT: OnceLock<Mutex<(wgpu::Device, Engine)>> = OnceLock::new();
    let context = CONTEXT.get_or_init(|| {
        iced::futures::executor::block_on(async {
            let instance = wgpu::Instance::default();
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions::default())
                .await
                .expect("headless WGPU test adapter");
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor::default(), None)
                .await
                .expect("headless WGPU test device");
            let engine = Engine::new(
                &adapter,
                &device,
                &queue,
                wgpu::TextureFormat::Bgra8UnormSrgb,
                None,
            );
            Mutex::new((device, engine))
        })
    });
    let context = context.lock().expect("headless renderer context");
    iced::Renderer::new(&context.0, &context.1, font, 16.0.into())
}
