use crate::classifier::{ShotClassification, ShotLabel};
use crate::config::AppConfig;
use crate::input;
use egui::{Align, Align2, Color32, FontId, Frame, Label, Layout, Margin, RichText, Sense, Stroke};
use egui_wgpu::{Renderer, ScreenDescriptor};
use std::error::Error;
use std::sync::Arc;
use wgpu::CurrentSurfaceTexture;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::monitor::MonitorHandle;
use winit::window::{Fullscreen, Window, WindowAttributes, WindowId, WindowLevel};

const INITIAL_WIDTH: f64 = 260.0;
const INITIAL_HEIGHT: f64 = 96.0;
const MIN_BODY_FONT_SIZE: f32 = 8.0;
const MAX_BODY_FONT_SIZE: f32 = 24.0;

#[derive(Clone, Debug, PartialEq)]
pub enum UiCommand {
    Shot(ShotClassification),
    ToggleVisible,
    ToggleSecondDisplayFullscreen,
    IncreaseSize,
    DecreaseSize,
    Exit,
}

pub fn run(config: AppConfig) -> Result<(), Box<dyn Error>> {
    let mut event_loop_builder = EventLoop::<UiCommand>::with_user_event();
    let event_loop = event_loop_builder.build()?;
    input::start_input_listener(event_loop.create_proxy(), config.movement);

    let mut app = OverlayApplication::default();
    event_loop.run_app(&mut app)?;

    Ok(())
}

#[derive(Default)]
struct OverlayApplication {
    window: Option<Arc<Window>>,
    egui_ctx: egui::Context,
    egui_state: Option<egui_winit::State>,
    graphics: Option<GraphicsState>,
    overlay: OverlayState,
    is_fullscreen: bool,
}

impl ApplicationHandler<UiCommand> for OverlayApplication {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        if let Err(error) = self.create_window(event_loop) {
            log::error!("failed to create overlay window: {error}");
            event_loop.exit();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window.as_ref() else {
            return;
        };

        if window.id() != window_id {
            return;
        }

        if let Some(egui_state) = self.egui_state.as_mut() {
            let response = egui_state.on_window_event(window, &event);
            if response.repaint {
                window.request_redraw();
            }
        }

        match event {
            WindowEvent::CloseRequested => {
                log::info!("window close requested; exiting");
                event_loop.exit();
            }
            WindowEvent::Destroyed => {
                log::info!("window destroyed; exiting");
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(graphics) = self.graphics.as_mut() {
                    graphics.resize(size);
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(graphics) = self.graphics.as_mut() {
                    graphics.resize(window.inner_size());
                }
            }
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            _ => {}
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UiCommand) {
        match event {
            UiCommand::Shot(result) => self.overlay.last_result = Some(result),
            UiCommand::ToggleVisible => self.toggle_visibility(),
            UiCommand::ToggleSecondDisplayFullscreen => {
                self.toggle_second_display_fullscreen(event_loop)
            }
            UiCommand::IncreaseSize => self.overlay.increase_size(),
            UiCommand::DecreaseSize => self.overlay.decrease_size(),
            UiCommand::Exit => {
                log::info!("exit hotkey pressed; exiting");
                event_loop.exit();
                return;
            }
        }

        if !self.is_fullscreen {
            self.apply_desired_window_size();
        }

        if let Some(window) = self.window.as_ref()
            && self.overlay.is_visible
        {
            window.request_redraw();
        }
    }
}

impl OverlayApplication {
    fn create_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Box<dyn Error>> {
        let attributes = WindowAttributes::default()
            .with_title("dStrafe")
            .with_inner_size(LogicalSize::new(INITIAL_WIDTH, INITIAL_HEIGHT))
            .with_decorations(false)
            .with_resizable(false)
            .with_window_level(WindowLevel::AlwaysOnTop);
        let window = Arc::new(event_loop.create_window(attributes)?);
        window.set_window_level(WindowLevel::AlwaysOnTop);

        let graphics = pollster::block_on(GraphicsState::new(window.clone(), event_loop))?;
        let egui_state = egui_winit::State::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            event_loop,
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(graphics.max_texture_side()),
        );

        self.graphics = Some(graphics);
        self.egui_state = Some(egui_state);
        self.window = Some(window.clone());
        window.request_redraw();

        Ok(())
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let (Some(window), Some(egui_state), Some(graphics)) = (
            self.window.as_ref(),
            self.egui_state.as_mut(),
            self.graphics.as_mut(),
        ) else {
            return;
        };

        if !self.overlay.is_visible {
            return;
        }

        match graphics.render(
            window,
            &self.egui_ctx,
            egui_state,
            &mut self.overlay,
            self.is_fullscreen,
        ) {
            Ok(()) => {}
            Err(RenderAction::SkipFrame) => {}
            Err(RenderAction::Reconfigure) => {
                if graphics.resize(window.inner_size()) {
                    window.request_redraw();
                }
            }
            Err(RenderAction::RecreateSurface) => match graphics.recreate_surface(window.clone()) {
                Ok(true) => window.request_redraw(),
                Ok(false) => {}
                Err(error) => {
                    log::error!("failed to recreate surface: {error}");
                    event_loop.exit();
                }
            },
        }
    }

    fn toggle_visibility(&mut self) {
        self.overlay.is_visible = !self.overlay.is_visible;

        if let Some(window) = self.window.as_ref() {
            window.set_visible(self.overlay.is_visible);
        }
    }

    fn toggle_second_display_fullscreen(&mut self, event_loop: &ActiveEventLoop) {
        let Some(window) = self.window.as_ref() else {
            return;
        };

        if self.is_fullscreen {
            window.set_fullscreen(None);
            self.is_fullscreen = false;
            return;
        }

        let Some(monitor) = second_display_monitor(event_loop) else {
            log::warn!("Ctrl+F7 ignored: no second display is available");
            return;
        };

        let position = monitor.position();
        let display_name = monitor.name().unwrap_or_else(|| "display 2".to_owned());
        log::info!("entering fullscreen on {display_name}");
        window.set_outer_position(position);
        window.set_fullscreen(Some(Fullscreen::Borderless(Some(monitor))));
        self.is_fullscreen = true;
    }

    fn apply_desired_window_size(&self) {
        if let Some(window) = self.window.as_ref() {
            let (width, height) = self.overlay.desired_window_size();
            let _ = window.request_inner_size(LogicalSize::new(width, height));
        }
    }
}

struct GraphicsState {
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    size: PhysicalSize<u32>,
    surface_configured: bool,
}

impl GraphicsState {
    async fn new(
        window: Arc<Window>,
        event_loop: &ActiveEventLoop,
    ) -> Result<Self, Box<dyn Error>> {
        let size = non_zero_size(window.inner_size());
        let mut instance_descriptor = wgpu::InstanceDescriptor::new_with_display_handle(Box::new(
            event_loop.owned_display_handle(),
        ));
        instance_descriptor.backends =
            wgpu::Backends::from_env().unwrap_or_else(preferred_backends);
        let instance = wgpu::Instance::new(instance_descriptor);
        let surface = instance.create_surface(window)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await?;
        let adapter_info = adapter.get_info();
        log::info!(
            "using {:?} backend adapter: {}",
            adapter_info.backend,
            adapter_info.name
        );
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("dStrafe device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            })
            .await?;
        device.on_uncaptured_error(Arc::new(|error| {
            log::error!("uncaptured wgpu error: {error:#?}");
        }));
        let config = surface_config(&surface, &adapter, size)?;
        configure_surface_checked(&surface, &device, &config).map_err(|error| {
            std::io::Error::other(format!("failed to configure surface: {error}"))
        })?;
        let renderer = Renderer::new(
            &device,
            config.format,
            egui_wgpu::RendererOptions::default(),
        );

        Ok(Self {
            instance,
            surface,
            device,
            queue,
            config,
            renderer,
            size,
            surface_configured: true,
        })
    }

    fn max_texture_side(&self) -> usize {
        self.device.limits().max_texture_dimension_2d as usize
    }

    fn resize(&mut self, size: PhysicalSize<u32>) -> bool {
        let size = non_zero_size(size);
        self.size = size;
        self.config.width = size.width;
        self.config.height = size.height;
        self.configure_surface()
    }

    fn recreate_surface(&mut self, window: Arc<Window>) -> Result<bool, wgpu::CreateSurfaceError> {
        self.surface_configured = false;
        let size = non_zero_size(window.inner_size());
        self.size = size;
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface = self.instance.create_surface(window)?;
        Ok(self.configure_surface())
    }

    fn configure_surface(&mut self) -> bool {
        match configure_surface_checked(&self.surface, &self.device, &self.config) {
            Ok(()) => {
                self.surface_configured = true;
                true
            }
            Err(error) => {
                log::warn!("surface configure failed: {error}");
                self.surface_configured = false;
                false
            }
        }
    }

    fn render(
        &mut self,
        window: &Window,
        egui_ctx: &egui::Context,
        egui_state: &mut egui_winit::State,
        overlay: &mut OverlayState,
        is_fullscreen: bool,
    ) -> Result<(), RenderAction> {
        let window_size = non_zero_size(window.inner_size());
        if self.size != window_size {
            self.resize(window_size);
        }

        if !self.surface_configured && !self.configure_surface() {
            return Err(RenderAction::SkipFrame);
        }

        let raw_input = egui_state.take_egui_input(window);
        let output = egui_ctx.run_ui(raw_input, |ui| overlay.ui(ui, window, is_fullscreen));
        egui_state.handle_platform_output(window, output.platform_output);

        let pixels_per_point = egui_winit::pixels_per_point(egui_ctx, window);
        let clipped_primitives = egui_ctx.tessellate(output.shapes, pixels_per_point);
        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [self.size.width, self.size.height],
            pixels_per_point,
        };

        for (id, image_delta) in &output.textures_delta.set {
            self.renderer
                .update_texture(&self.device, &self.queue, *id, image_delta);
        }

        let (frame, is_suboptimal) = match self.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(frame) => (frame, false),
            CurrentSurfaceTexture::Suboptimal(frame) => (frame, true),
            CurrentSurfaceTexture::Timeout | CurrentSurfaceTexture::Occluded => {
                return Err(RenderAction::SkipFrame);
            }
            CurrentSurfaceTexture::Outdated => return Err(RenderAction::Reconfigure),
            CurrentSurfaceTexture::Lost => return Err(RenderAction::RecreateSurface),
            CurrentSurfaceTexture::Validation => {
                self.surface_configured = false;
                log::warn!("surface validation failed; recreating surface");
                return Err(RenderAction::RecreateSurface);
            }
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("dStrafe render encoder"),
            });
        let callback_buffers = self.renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &clipped_primitives,
            &screen_descriptor,
        );

        {
            let mut render_pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("dStrafe egui render pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.125,
                                g: 0.125,
                                b: 0.125,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            self.renderer
                .render(&mut render_pass, &clipped_primitives, &screen_descriptor);
        }

        self.queue
            .submit(callback_buffers.into_iter().chain([encoder.finish()]));
        frame.present();

        for id in &output.textures_delta.free {
            self.renderer.free_texture(id);
        }

        if is_suboptimal {
            return Err(RenderAction::Reconfigure);
        }

        Ok(())
    }
}

#[derive(Debug)]
enum RenderAction {
    SkipFrame,
    Reconfigure,
    RecreateSurface,
}

fn surface_config(
    surface: &wgpu::Surface<'static>,
    adapter: &wgpu::Adapter,
    size: PhysicalSize<u32>,
) -> Result<wgpu::SurfaceConfiguration, Box<dyn Error>> {
    let capabilities = surface.get_capabilities(adapter);
    let format = capabilities
        .formats
        .iter()
        .copied()
        .find(|format| {
            matches!(
                format,
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
            )
        })
        .or_else(|| capabilities.formats.first().copied())
        .ok_or_else(|| std::io::Error::other("surface has no supported formats"))?;
    let present_mode = capabilities
        .present_modes
        .iter()
        .copied()
        .find(|mode| *mode == wgpu::PresentMode::Fifo)
        .or_else(|| capabilities.present_modes.first().copied())
        .ok_or_else(|| std::io::Error::other("surface has no supported present modes"))?;
    let alpha_mode = capabilities
        .alpha_modes
        .iter()
        .copied()
        .find(|mode| *mode == wgpu::CompositeAlphaMode::Auto)
        .or_else(|| capabilities.alpha_modes.first().copied())
        .ok_or_else(|| std::io::Error::other("surface has no supported alpha modes"))?;

    Ok(wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: size.width,
        height: size.height,
        present_mode,
        desired_maximum_frame_latency: 2,
        alpha_mode,
        view_formats: vec![format],
    })
}

fn non_zero_size(size: PhysicalSize<u32>) -> PhysicalSize<u32> {
    PhysicalSize::new(size.width.max(1), size.height.max(1))
}

fn preferred_backends() -> wgpu::Backends {
    if cfg!(target_os = "windows") {
        wgpu::Backends::DX12
    } else {
        wgpu::Backends::PRIMARY
    }
}

fn configure_surface_checked(
    surface: &wgpu::Surface<'static>,
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
) -> Result<(), String> {
    let out_of_memory_scope = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let internal_scope = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let validation_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);

    surface.configure(device, config);

    let mut errors = Vec::new();
    if let Some(error) = pollster::block_on(validation_scope.pop()) {
        errors.push(format!("{error:#?}"));
    }
    if let Some(error) = pollster::block_on(internal_scope.pop()) {
        errors.push(format!("{error:#?}"));
    }
    if let Some(error) = pollster::block_on(out_of_memory_scope.pop()) {
        errors.push(format!("{error:#?}"));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn second_display_monitor(event_loop: &ActiveEventLoop) -> Option<MonitorHandle> {
    let monitors = event_loop.available_monitors().collect::<Vec<_>>();
    if monitors.len() < 2 {
        return None;
    }

    if let Some(primary) = event_loop.primary_monitor()
        && let Some(monitor) = monitors.iter().find(|monitor| **monitor != primary)
    {
        return Some(monitor.clone());
    }

    monitors.get(1).cloned()
}

struct OverlayState {
    is_visible: bool,
    header_font_size: f32,
    body_font_size: f32,
    last_result: Option<ShotClassification>,
}

impl Default for OverlayState {
    fn default() -> Self {
        Self {
            is_visible: true,
            header_font_size: 12.0,
            body_font_size: 10.0,
            last_result: None,
        }
    }
}

impl OverlayState {
    fn ui(&mut self, ui: &mut egui::Ui, window: &Window, is_fullscreen: bool) {
        if self.header_font_size == 0.0 {
            self.header_font_size = 12.0;
        }
        if self.body_font_size == 0.0 {
            self.body_font_size = 10.0;
        }

        if is_fullscreen {
            self.fullscreen_ui(ui);
        } else {
            self.compact_ui(ui, window);
        }
    }

    fn compact_ui(&mut self, ui: &mut egui::Ui, window: &Window) {
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(self.background_color())
                    .stroke(Stroke::new(2.0, Color32::from_rgb(12, 12, 12)))
                    .inner_margin(Margin::ZERO),
            )
            .show_inside(ui, |ui| {
                let header_height = (self.header_font_size + 12.0).round();
                let header_width = ui.available_width();
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(header_width, header_height), Sense::drag());

                ui.painter()
                    .rect_filled(rect, 0.0, Color32::from_rgb(48, 48, 48));
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    "dStrafe",
                    FontId::monospace(self.header_font_size),
                    Color32::WHITE,
                );

                if response.drag_started()
                    && let Err(error) = window.drag_window()
                {
                    log::debug!("window drag request failed: {error}");
                }

                ui.add_space(4.0);
                ui.with_layout(
                    Layout::centered_and_justified(egui::Direction::TopDown),
                    |ui| {
                        let text = self.last_result.as_ref().map_or_else(
                            || "Waiting for input...".to_owned(),
                            |result| result.display_text(),
                        );
                        let label = Label::new(
                            RichText::new(text)
                                .font(FontId::monospace(self.body_font_size))
                                .color(Color32::WHITE),
                        )
                        .selectable(false)
                        .halign(Align::Center);
                        ui.add(label);
                    },
                );
            });
    }

    fn fullscreen_ui(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(self.background_color())
                    .inner_margin(Margin::ZERO),
            )
            .show_inside(ui, |ui| {
                let header_height = 56.0;
                let header_width = ui.available_width();
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(header_width, header_height), Sense::hover());

                ui.painter()
                    .rect_filled(rect, 0.0, Color32::from_rgb(48, 48, 48));
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    "dStrafe",
                    FontId::monospace(24.0),
                    Color32::WHITE,
                );

                let text = self.display_text();
                let font_size = fullscreen_font_size(&text, ui.available_size());
                ui.allocate_ui_with_layout(
                    ui.available_size(),
                    Layout::centered_and_justified(egui::Direction::TopDown),
                    |ui| {
                        let label = Label::new(
                            RichText::new(text)
                                .font(FontId::monospace(font_size))
                                .color(Color32::WHITE),
                        )
                        .selectable(false)
                        .halign(Align::Center);
                        ui.add(label);
                    },
                );
            });
    }

    fn increase_size(&mut self) {
        self.ensure_font_sizes();
        if self.body_font_size < MAX_BODY_FONT_SIZE {
            self.body_font_size += 2.0;
            self.header_font_size += 2.0;
        }
    }

    fn decrease_size(&mut self) {
        self.ensure_font_sizes();
        if self.body_font_size > MIN_BODY_FONT_SIZE {
            self.body_font_size -= 2.0;
            self.header_font_size = (self.header_font_size - 2.0).max(10.0);
        }
    }

    fn desired_window_size(&self) -> (f64, f64) {
        let body = if self.body_font_size == 0.0 {
            10.0
        } else {
            self.body_font_size
        };
        let header = if self.header_font_size == 0.0 {
            12.0
        } else {
            self.header_font_size
        };
        let max_line_len = self
            .last_result
            .as_ref()
            .map(|result| {
                result
                    .display_text()
                    .lines()
                    .map(str::len)
                    .max()
                    .unwrap_or("Waiting for input...".len())
            })
            .unwrap_or("Waiting for input...".len());
        let width = (max_line_len as f64 * body as f64 * 0.68 + 52.0).max(INITIAL_WIDTH);
        let height = (header as f64 + body as f64 * 4.0 + 38.0).max(INITIAL_HEIGHT);

        (width, height)
    }

    fn display_text(&self) -> String {
        self.last_result.as_ref().map_or_else(
            || "Waiting for input...".to_owned(),
            |result| result.display_text(),
        )
    }

    fn background_color(&self) -> Color32 {
        match self.last_result.as_ref().map(|result| result.label) {
            Some(ShotLabel::CounterStrafe) => Color32::from_rgb(34, 139, 34),
            Some(ShotLabel::Overlap) => Color32::from_rgb(255, 140, 0),
            Some(ShotLabel::Bad) => Color32::from_rgb(204, 0, 0),
            None => Color32::from_rgb(32, 32, 32),
        }
    }

    fn ensure_font_sizes(&mut self) {
        if self.header_font_size == 0.0 {
            self.header_font_size = 12.0;
        }
        if self.body_font_size == 0.0 {
            self.body_font_size = 10.0;
        }
    }
}

fn fullscreen_font_size(text: &str, available_size: egui::Vec2) -> f32 {
    let max_line_len = text.lines().map(str::len).max().unwrap_or(1).max(1) as f32;
    let line_count = text.lines().count().max(1) as f32;
    let width_limited = available_size.x * 0.88 / (max_line_len * 0.62);
    let height_limited = available_size.y * 0.72 / (line_count * 1.2);

    width_limited.min(height_limited).clamp(20.0, 140.0)
}
