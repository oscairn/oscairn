// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

//! This module contains the window adapter implementation to communicate between Slint and Vulkan + libinput

use std::cell::{Cell, RefCell};
use std::pin::Pin;
use std::rc::Rc;

use i_slint_core::Property;
use i_slint_core::api::{LogicalPosition, PhysicalSize as PhysicalWindowSize};
use i_slint_core::cursor::MouseCursorInner;
use i_slint_core::graphics::{Image, euclid};
use i_slint_core::item_rendering::ItemRenderer;
use i_slint_core::items::BuiltInMouseCursor;
use i_slint_core::lengths::{LogicalRect, LogicalVector};
use i_slint_core::platform::WindowEvent;
use i_slint_core::renderer::DrawOutcome;
use i_slint_core::slice::Slice;
use i_slint_core::{platform::PlatformError, window::WindowAdapter, window::WindowAdapterInternal};

use crate::display::RenderingRotation;

pub trait FullscreenRenderer {
    fn as_core_renderer(&self) -> &dyn i_slint_core::renderer::Renderer;
    fn render_and_present(
        &self,
        rotation: RenderingRotation,
        draw_mouse_cursor_callback: &dyn Fn(&mut dyn ItemRenderer),
    ) -> Result<DrawOutcome, PlatformError>;
    fn size(&self) -> PhysicalWindowSize;
}

pub struct FullscreenWindowAdapter {
    window: i_slint_core::api::Window,
    renderer: Box<dyn FullscreenRenderer>,
    redraw_requested: Cell<bool>,
    rotation: RenderingRotation,
    loop_signal: RefCell<Option<calloop::LoopSignal>>,
    mouse_cursor: RefCell<MouseCursorInner>,
    /// SLINT_KMS_MAX_FPS: the shortest time between two frame starts (None: render on every request).
    min_frame_interval: Option<std::time::Duration>,
    last_frame_start: Cell<Option<std::time::Instant>>,
    cap_timer_armed: Cell<bool>,
    /// SLINT_KMS_FRAME_STATS: per-frame (start, render+present duration) of the current burst of frames.
    frame_log: RefCell<Option<Vec<(std::time::Instant, std::time::Duration)>>>,
    frame_log_timer: i_slint_core::timers::Timer,
}

impl WindowAdapter for FullscreenWindowAdapter {
    fn window(&self) -> &i_slint_core::api::Window {
        &self.window
    }

    fn size(&self) -> i_slint_core::api::PhysicalSize {
        self.rotation.screen_size_to_rotated_window_size(self.renderer.size())
    }

    fn renderer(&self) -> &dyn i_slint_core::renderer::Renderer {
        self.renderer.as_core_renderer()
    }

    fn request_redraw(&self) {
        self.redraw_requested.set(true);
        if let Some(signal) = self.loop_signal.borrow().as_ref() {
            signal.wakeup();
        }
    }

    fn set_visible(&self, visible: bool) -> Result<(), PlatformError> {
        if visible
            && let Some(scale_factor) =
                std::env::var("SLINT_SCALE_FACTOR").ok().and_then(|sf| sf.parse().ok())
        {
            self.window
                .dispatch_event_with_result(WindowEvent::ScaleFactorChanged { scale_factor })?;
        }
        Ok(())
    }

    fn internal(&self, _: i_slint_core::InternalToken) -> Option<&dyn WindowAdapterInternal> {
        Some(self)
    }
}

impl WindowAdapterInternal for FullscreenWindowAdapter {
    fn set_mouse_cursor(&self, cursor: MouseCursorInner) {
        if *self.mouse_cursor.borrow() != cursor {
            *self.mouse_cursor.borrow_mut() = cursor;
            self.request_redraw();
        }
    }
}

impl FullscreenWindowAdapter {
    pub fn new(
        renderer: Box<dyn FullscreenRenderer>,
        rotation: RenderingRotation,
    ) -> Result<Rc<Self>, PlatformError> {
        // Oscairn: at a cold start the UI once opened the display and never got a frame on it (the
        // console stayed on the screen); started again it drew at once. So a UI with no frame on
        // the glass within OSCAIRN_FIRST_FRAME_SECS (8) exits, and the start script's keeper starts it again.
        static WATCH: std::sync::Once = std::sync::Once::new();
        WATCH.call_once(|| {
            let secs = std::env::var("OSCAIRN_FIRST_FRAME_SECS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(8);
            if secs > 0 {
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(secs));
                    if !FIRST_FRAME_SHOWN.load(std::sync::atomic::Ordering::Acquire) {
                        eprintln!("oscairn: no frame on the display within {secs} s: ending, so that the keeper starts the UI again");
                        std::process::exit(4);
                    }
                });
            }
        });
        let size = renderer.size();
        let rotation_degrees = rotation.degrees();
        eprintln!(
            "Rendering at {}x{}{}",
            size.width,
            size.height,
            if rotation_degrees != 0. {
                format!(" with {} rotation_degrees rotation", rotation_degrees)
            } else {
                String::new()
            }
        );
        Ok(Rc::<FullscreenWindowAdapter>::new_cyclic(|self_weak| FullscreenWindowAdapter {
            window: i_slint_core::api::Window::new(self_weak.clone()),
            renderer,
            redraw_requested: Cell::new(true),
            rotation,
            loop_signal: RefCell::new(None),
            mouse_cursor: RefCell::new(MouseCursorInner::default()),
            min_frame_interval: std::env::var("SLINT_KMS_MAX_FPS")
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .filter(|fps| *fps > 0.)
                // A little under the period, so that frames land on every n-th vblank rather than
                // slipping to the next one when the timer wakes late.
                .map(|fps| std::time::Duration::from_secs_f64((1. / fps - 0.002).max(0.))),
            last_frame_start: Cell::new(None),
            cap_timer_armed: Cell::new(false),
            frame_log: RefCell::new(std::env::var_os("SLINT_KMS_FRAME_STATS").map(|_| Vec::new())),
            frame_log_timer: Default::default(),
        }))
    }

    pub fn set_loop_signal(&self, signal: calloop::LoopSignal) {
        *self.loop_signal.borrow_mut() = Some(signal);
    }

    pub fn render_if_needed(
        self: Rc<Self>,
        mouse_position: Pin<&Property<Option<LogicalPosition>>>,
    ) -> Result<(), PlatformError> {
        if self.redraw_requested.get() {
            let now = std::time::Instant::now();
            if let (Some(min), Some(last)) = (self.min_frame_interval, self.last_frame_start.get()) {
                let elapsed = now - last;
                if elapsed < min {
                    // Too early for the frame cap: keep the request and come back when it is time.
                    if !self.cap_timer_armed.replace(true) {
                        let self_weak = Rc::downgrade(&self);
                        i_slint_core::timers::Timer::single_shot(min - elapsed, move || {
                            if let Some(this) = self_weak.upgrade() {
                                this.cap_timer_armed.set(false);
                                this.request_redraw();
                            }
                        });
                    }
                    return Ok(());
                }
            }
            self.redraw_requested.set(false);
            self.last_frame_start.set(Some(now));
            let outcome = self.renderer.render_and_present(self.rotation, &|item_renderer| {
                if let Some(mouse_position) = mouse_position.get() {
                    // The image is drawn at its natural size, so the hotspot is just clamped into it.
                    let image_and_hotspot = match &*self.mouse_cursor.borrow() {
                        MouseCursorInner::CustomMouseCursor { image, hotspot_x, hotspot_y } => {
                            let size = image.size();
                            let hotspot = LogicalVector::new(
                                (*hotspot_x).clamp(0, size.width.saturating_sub(1) as i32) as f32,
                                (*hotspot_y).clamp(0, size.height.saturating_sub(1) as i32) as f32,
                            );
                            Some((image.clone(), hotspot))
                        }
                        // Only the `none` shape hides the cursor; every other built-in shape falls
                        // back to the default pointer, as this backend has a single cursor image.
                        MouseCursorInner::BuiltIn(builtin) => {
                            (!matches!(builtin, BuiltInMouseCursor::None))
                                .then(|| (mouse_cursor_image(), LogicalVector::default()))
                        }
                        _ => Some((mouse_cursor_image(), LogicalVector::default())),
                    };
                    if let Some((cursor_image, hotspot)) = image_and_hotspot {
                        let origin = i_slint_core::lengths::logical_point_from_api(mouse_position)
                            .to_vector()
                            - hotspot;
                        item_renderer.save_state();
                        item_renderer.translate(origin);
                        item_renderer.draw_image_direct(cursor_image.clone());
                        item_renderer.restore_state();
                        let cursor_rect = LogicalRect::new(
                            origin.to_point(),
                            euclid::Size2D::from_untyped(cursor_image.size().cast()),
                        );
                        self.renderer.as_core_renderer().mark_dirty_region(cursor_rect.into());
                    }
                }
            })?;
            if !matches!(outcome, DrawOutcome::Success) {
                self.redraw_requested.set(true);
            } else {
                FIRST_FRAME_SHOWN.store(true, std::sync::atomic::Ordering::Release);
            }
            self.log_frame(now);
            // Check once after rendering if we have running animations and
            // remember that to trigger a redraw after the frame is on the screen.
            // Timers might have been updated if the event loop is woken up
            // due to other reasons, which would also reset has_active_animations.
            if self.window.has_active_animations() {
                let self_weak = Rc::downgrade(&self);
                i_slint_core::timers::Timer::single_shot(
                    std::time::Duration::default(),
                    move || {
                        let Some(this) = self_weak.upgrade() else {
                            return;
                        };
                        this.request_redraw();
                    },
                )
            }
        }
        Ok(())
    }
}

/// Oscairn: set once a frame has been presented (the first-frame watch in `new`).
static FIRST_FRAME_SHOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl FullscreenWindowAdapter {
    fn log_frame(self: &Rc<Self>, start: std::time::Instant) {
        static FIRST: std::sync::Once = std::sync::Once::new();
        FIRST.call_once(|| {
            let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
            eprintln!("oscairn-timing: shell first frame ms={ms}");
        });
        let mut log = self.frame_log.borrow_mut();
        let Some(log) = log.as_mut() else { return };
        log.push((start, start.elapsed()));
        let self_weak = Rc::downgrade(self);
        // A burst ends when no frame follows for 400 ms: print its numbers then.
        self.frame_log_timer.start(
            i_slint_core::timers::TimerMode::SingleShot,
            std::time::Duration::from_millis(400),
            move || {
                if let Some(this) = self_weak.upgrade() {
                    if let Some(log) = this.frame_log.borrow_mut().as_mut() {
                        print_frame_stats(log);
                        log.clear();
                    }
                }
            },
        );
    }
}

fn print_frame_stats(log: &[(std::time::Instant, std::time::Duration)]) {
    if log.len() < 2 {
        return;
    }
    let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.;
    let pct = |v: &mut Vec<f64>, p: f64| {
        v.sort_by(f64::total_cmp);
        v[((v.len() - 1) as f64 * p).round() as usize]
    };
    let mut gaps: Vec<f64> = log.windows(2).map(|w| ms(w[1].0 - w[0].0)).collect();
    let mut work: Vec<f64> = log.iter().map(|f| ms(f.1)).collect();
    let span = (log[log.len() - 1].0 - log[0].0).as_secs_f64();
    let n = log.len() as f64;
    let mean = gaps.iter().sum::<f64>() / gaps.len() as f64;
    let sd = (gaps.iter().map(|g| (g - mean) * (g - mean)).sum::<f64>() / gaps.len() as f64).sqrt();
    eprintln!(
        "kms-frames: {} frames in {:.2} s ({:.1}/s); interval ms mean {:.1} sd {:.1} p50 {:.1} p90 {:.1} p99 {:.1} max {:.1}; render+present ms p50 {:.1} p90 {:.1} max {:.1}",
        log.len(), span, (n - 1.) / span.max(1e-9), mean, sd,
        pct(&mut gaps, 0.5), pct(&mut gaps, 0.9), pct(&mut gaps, 0.99), pct(&mut gaps, 1.0),
        pct(&mut work, 0.5), pct(&mut work, 0.9), pct(&mut work, 1.0),
    );
}

fn mouse_cursor_image() -> Image {
    let mouse_pointer_svg = i_slint_core::graphics::load_image_from_embedded_data(
        Slice::from_slice(include_bytes!("mouse-pointer.svg")),
        Slice::from_slice(b"svg"),
    );
    let mouse_pointer_inner: &i_slint_core::graphics::ImageInner = (&mouse_pointer_svg).into();
    match mouse_pointer_inner {
        i_slint_core::ImageInner::Svg(svg) => {
            let pixels = svg.render(None).unwrap();
            let cache_key = svg.cache_key();
            let mouse_pointer_pixel_image = i_slint_core::graphics::ImageInner::EmbeddedImage {
                cache_key: cache_key.clone(),
                buffer: pixels,
            };
            i_slint_core::graphics::cache::replace_cached_image(
                cache_key,
                mouse_pointer_pixel_image.clone(),
            );

            mouse_pointer_pixel_image.into()
        }
        cached_image => cached_image.clone().into(),
    }
}
