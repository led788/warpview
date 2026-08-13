use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::config::{self, WindowConfig};
use crate::decode::{self, DecodedImage};
use crate::dirscan::{self, DirListing};
use crate::render::Renderer;

const CACHE_CAPACITY: usize = 5;

pub enum UserEvent {
    Decoded(usize, DecodedImage),
}

pub struct App {
    initial_path: PathBuf,
    proxy: EventLoopProxy<UserEvent>,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    listing: Option<DirListing>,
    cache: HashMap<usize, DecodedImage>,
    cache_order: VecDeque<usize>,
    pending: HashSet<usize>,
    frame_index: usize,
    next_frame_at: Option<Instant>,
}

impl App {
    pub fn new(initial_path: PathBuf, proxy: EventLoopProxy<UserEvent>) -> Self {
        Self {
            initial_path,
            proxy,
            window: None,
            renderer: None,
            listing: None,
            cache: HashMap::new(),
            cache_order: VecDeque::new(),
            pending: HashSet::new(),
            frame_index: 0,
            next_frame_at: None,
        }
    }

    fn apply_current(&mut self) {
        let Some(listing) = &self.listing else { return };
        let idx = listing.current_index;
        let Some(img) = self.cache.get(&idx) else { return };
        if let Some(renderer) = &mut self.renderer {
            renderer.set_image(img);
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        self.update_title();

        self.frame_index = 0;
        self.next_frame_at = if img.is_animated() {
            Some(Instant::now() + img.frames[0].delay)
        } else {
            None
        };
    }

    /// Advance to the next frame of the currently displayed animated
    /// image (GIF/APNG) and schedule the following one.
    fn advance_frame(&mut self) {
        let Some(listing) = &self.listing else { return };
        let idx = listing.current_index;
        let Some(img) = self.cache.get(&idx) else {
            self.next_frame_at = None;
            return;
        };
        if !img.is_animated() {
            self.next_frame_at = None;
            return;
        }
        self.frame_index = (self.frame_index + 1) % img.frames.len();
        let frame = &img.frames[self.frame_index];
        if let Some(renderer) = &mut self.renderer {
            renderer.update_frame_pixels(&frame.rgba);
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        self.next_frame_at = Some(Instant::now() + frame.delay);
    }

    fn update_title(&self) {
        let Some(window) = &self.window else { return };
        let Some(listing) = &self.listing else { return };
        if listing.files.is_empty() {
            window.set_title("WarpView");
            return;
        }
        let name = listing.files[listing.current_index]
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        window.set_title(&format!(
            "{name} ({} of {}) — WarpView",
            listing.current_index + 1,
            listing.files.len()
        ));
    }

    fn navigate(&mut self, forward: bool) {
        let (idx, path) = {
            let Some(listing) = &mut self.listing else { return };
            if listing.files.is_empty() {
                return;
            }
            if forward {
                listing.next();
            } else {
                listing.prev();
            }
            (
                listing.current_index,
                listing.files[listing.current_index].clone(),
            )
        };

        if self.cache.contains_key(&idx) {
            self.apply_current();
        } else {
            match decode::decode(&path) {
                Ok(img) => {
                    self.cache.insert(idx, img);
                    self.cache_order.push_back(idx);
                    self.apply_current();
                }
                Err(e) => eprintln!("failed to decode {}: {e}", path.display()),
            }
        }
        self.prefetch_neighbors();
        self.evict_cache();
    }

    fn prefetch_neighbors(&mut self) {
        let Some(listing) = &self.listing else { return };
        let len = listing.files.len();
        if len <= 1 {
            return;
        }
        let idx = listing.current_index;
        let next_idx = (idx + 1) % len;
        let prev_idx = (idx + len - 1) % len;
        for target in [next_idx, prev_idx] {
            if self.cache.contains_key(&target) || self.pending.contains(&target) {
                continue;
            }
            self.pending.insert(target);
            let path = listing.files[target].clone();
            let proxy = self.proxy.clone();
            std::thread::spawn(move || {
                if let Ok(img) = decode::decode(&path) {
                    let _ = proxy.send_event(UserEvent::Decoded(target, img));
                }
            });
        }
    }

    fn evict_cache(&mut self) {
        let current = self.listing.as_ref().map(|l| l.current_index);
        while self.cache.len() > CACHE_CAPACITY {
            let Some(oldest) = self.cache_order.pop_front() else {
                break;
            };
            if Some(oldest) == current {
                self.cache_order.push_back(oldest);
                if self.cache_order.iter().all(|&i| Some(i) == current) {
                    break;
                }
                continue;
            }
            self.cache.remove(&oldest);
        }
    }

    fn save_window_size(&self) {
        let Some(window) = &self.window else { return };
        let size = window.inner_size();
        config::save(WindowConfig {
            width: size.width,
            height: size.height,
        });
    }
}

fn default_size(event_loop: &ActiveEventLoop) -> (u32, u32) {
    if let Some(monitor) = event_loop.primary_monitor() {
        let size = monitor.size();
        let w = (size.width as f32 * 0.75) as u32;
        let h = (size.height as f32 * 0.75) as u32;
        (w.max(400), h.max(300))
    } else {
        (1280, 800)
    }
}

/// Taskbar/alt-tab window icon on Windows and Linux (macOS ignores
/// per-window icons and uses the app bundle icon instead).
fn load_window_icon() -> Option<winit::window::Icon> {
    const ICON_BYTES: &[u8] = include_bytes!("../assets/icon_256.png");
    let img = image::load_from_memory(ICON_BYTES).ok()?.into_rgba8();
    let (width, height) = img.dimensions();
    winit::window::Icon::from_rgba(img.into_raw(), width, height).ok()
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let (width, height) = match config::load() {
            Some(c) => (c.width, c.height),
            None => default_size(event_loop),
        };

        let position = event_loop.primary_monitor().map(|monitor| {
            let msize = monitor.size();
            let mpos = monitor.position();
            winit::dpi::PhysicalPosition::new(
                mpos.x + (msize.width as i32 - width as i32) / 2,
                mpos.y + (msize.height as i32 - height as i32) / 2,
            )
        });

        let mut attrs = Window::default_attributes()
            .with_title("WarpView")
            .with_inner_size(winit::dpi::PhysicalSize::new(width, height))
            .with_window_icon(load_window_icon());
        if let Some(position) = position {
            attrs = attrs.with_position(position);
        }

        let window = event_loop
            .create_window(attrs)
            .expect("failed to create window");
        let window = Arc::new(window);

        // Decode the first image in the background while wgpu spins up its
        // adapter/device (both take tens of milliseconds) so the two overlap.
        let decode_path = self.initial_path.clone();
        let decode_handle = std::thread::spawn(move || decode::decode(&decode_path));

        let renderer = Renderer::new(window.clone());
        let listing = dirscan::scan(&self.initial_path);

        self.window = Some(window.clone());
        self.renderer = Some(renderer);
        self.listing = Some(listing);

        match decode_handle.join().unwrap() {
            Ok(img) => {
                let idx = self.listing.as_ref().unwrap().current_index;
                self.cache.insert(idx, img);
                self.cache_order.push_back(idx);
                self.apply_current();
                self.prefetch_neighbors();
            }
            Err(e) => {
                eprintln!("failed to open {}: {e}", self.initial_path.display());
                event_loop.exit();
                return;
            }
        }

        window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
        let is_our_window = self.window.as_ref().is_some_and(|w| w.id() == window_id);
        if !is_our_window {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                self.save_window_size();
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.render();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    match event.logical_key {
                        Key::Named(NamedKey::Space) | Key::Named(NamedKey::ArrowRight) => {
                            self.navigate(true)
                        }
                        Key::Named(NamedKey::ArrowLeft) => self.navigate(false),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(at) = self.next_frame_at {
            if Instant::now() >= at {
                self.advance_frame();
            }
        }
        event_loop.set_control_flow(match self.next_frame_at {
            Some(at) => ControlFlow::WaitUntil(at),
            None => ControlFlow::Wait,
        });
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Decoded(idx, img) => {
                self.pending.remove(&idx);
                self.cache.insert(idx, img);
                self.cache_order.push_back(idx);
                self.evict_cache();
                if self.listing.as_ref().map(|l| l.current_index) == Some(idx) {
                    self.apply_current();
                }
            }
        }
    }
}
