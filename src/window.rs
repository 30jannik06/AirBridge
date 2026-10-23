// Live-Fenster: zeigt das jeweils neueste dekodierte Bild (winit = Fenster, softbuffer = Pixel zeichnen).
use crate::decoder::{Frame, Latest, Update};
use anyhow::Result;
use softbuffer::{Context, Surface};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Fullscreen, Icon, Window, WindowId};

const MAX_W: f64 = 1280.0;
const MAX_H: f64 = 900.0;

/// Der Event-Loop muss im Haupt-Thread erzeugt werden. Der Proxy weckt ihn aus anderen Threads auf.
pub fn create() -> Result<(EventLoop<()>, EventLoopProxy<()>)> {
    let event_loop = EventLoop::<()>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    Ok((event_loop, proxy))
}

pub fn run(event_loop: EventLoop<()>, latest: Latest) -> Result<()> {
    let mut app = App {
        latest,
        window: None,
        surface: None,
        frame: None,
        sized_for: (0, 0),
        last_click: None,
    };
    event_loop.run_app(&mut app)?; // blockiert, bis das Fenster geschlossen wird
    Ok(())
}

struct App {
    latest: Latest,
    window: Option<Arc<Window>>,
    surface: Option<Surface<Arc<Window>, Arc<Window>>>,
    frame: Option<Frame>,
    sized_for: (usize, usize), // für welche Bildgröße wir das Fenster zuletzt angepasst haben
    last_click: Option<Instant>, // für Doppelklick = Vollbild
}

impl ApplicationHandler<()> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_title("AirBridge – warte auf iPhone …")
            .with_window_icon(Icon::from_rgba(crate::icon::rgba(64), 64, 64).ok())
            .with_inner_size(LogicalSize::new(405.0, 720.0));
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("Fenster konnte nicht erstellt werden"),
        );
        let context = Context::new(window.clone()).expect("Grafikkontext");
        self.surface = Some(Surface::new(&context, window.clone()).expect("Zeichenfläche"));
        self.window = Some(window);
    }

    // Wird vom Decoder-Thread ausgelöst, wenn ein neues Bild bereitliegt.
    fn user_event(&mut self, _: &ActiveEventLoop, _: ()) {
        let Some(update) = self.latest.lock().unwrap().take() else {
            return;
        };
        let Some(window) = self.window.clone() else {
            return;
        };
        match update {
            Update::Frame(frame) => {
                let size = (frame.width, frame.height);
                if size != self.sized_for {
                    // Neue Bildgröße (z. B. iPhone gedreht): Fenster passend dimensionieren.
                    // In PHYSISCHEN Pixeln, damit bei Windows-Skalierung (125/150 %) 1 Bildpixel = 1 Bildschirmpixel
                    // bleibt. Nur wenn das Bild nicht auf den Monitor passt, wird verkleinert.
                    let (max_w, max_h) = window
                        .current_monitor()
                        .map(|m| (m.size().width as f64 * 0.95, m.size().height as f64 * 0.85))
                        .unwrap_or((MAX_W, MAX_H));
                    let scale = (max_w / size.0 as f64).min(max_h / size.1 as f64).min(1.0);
                    let _ = window.request_inner_size(PhysicalSize::new(
                        (size.0 as f64 * scale).round() as u32,
                        (size.1 as f64 * scale).round() as u32,
                    ));
                    window.set_title("AirBridge – live");
                    self.sized_for = size;
                }
                self.frame = Some(frame);
            }
            Update::Cleared => {
                self.frame = None;
                self.sized_for = (0, 0);
                window.set_title("AirBridge – warte auf iPhone …");
            }
        }
        window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested | WindowEvent::Resized(_) => self.draw(),
            // F11 = Vollbild an/aus, Esc = Vollbild verlassen
            WindowEvent::KeyboardInput { event: key, .. }
                if key.state == ElementState::Pressed && !key.repeat =>
            {
                match key.physical_key {
                    PhysicalKey::Code(KeyCode::F11) => self.set_fullscreen(!self.is_fullscreen()),
                    PhysicalKey::Code(KeyCode::Escape) if self.is_fullscreen() => {
                        self.set_fullscreen(false)
                    }
                    _ => {}
                }
            }
            // Doppelklick = Vollbild an/aus
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                let now = Instant::now();
                if self
                    .last_click
                    .is_some_and(|t| now.duration_since(t) < Duration::from_millis(400))
                {
                    self.set_fullscreen(!self.is_fullscreen());
                    self.last_click = None;
                } else {
                    self.last_click = Some(now);
                }
            }
            _ => {}
        }
    }
}

impl App {
    fn is_fullscreen(&self) -> bool {
        self.window
            .as_ref()
            .is_some_and(|w| w.fullscreen().is_some())
    }

    fn set_fullscreen(&self, on: bool) {
        if let Some(w) = &self.window {
            w.set_fullscreen(on.then(|| Fullscreen::Borderless(None)));
            w.set_cursor_visible(!on); // im Vollbild stört der Mauszeiger
        }
    }

    fn draw(&mut self) {
        let (Some(window), Some(surface)) = (&self.window, &mut self.surface) else {
            return;
        };
        let size = window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return;
        };
        if surface.resize(w, h).is_err() {
            return;
        }
        let Ok(mut buffer) = surface.buffer_mut() else {
            return;
        };
        buffer.fill(0x0010_1010); // dunkelgrauer Hintergrund / Letterbox

        if let Some(f) = &self.frame {
            let (bw, bh) = (size.width as usize, size.height as usize);
            // Bild unverzerrt einpassen (mit Rändern) und mit "nearest neighbour" skalieren.
            let scale = (bw as f64 / f.width as f64).min(bh as f64 / f.height as f64);
            let dw = ((f.width as f64 * scale) as usize).clamp(1, bw);
            let dh = ((f.height as f64 * scale) as usize).clamp(1, bh);
            let (ox, oy) = ((bw - dw) / 2, (bh - dh) / 2);
            if (dw, dh) == (f.width, f.height) {
                // 1:1 — keine Skalierung, Zeilen einfach kopieren.
                for y in 0..dh {
                    buffer[(oy + y) * bw + ox..][..dw]
                        .copy_from_slice(&f.pixels[y * f.width..][..f.width]);
                }
            } else {
                scale_bilinear(f, &mut buffer, bw, ox, oy, dw, dh);
            }
        }
        let _ = buffer.present();
    }
}

/// Mischt zwei 0x00RRGGBB-Pixel; t = 0..=256 (Anteil von b). Rot/Blau und Grün je in einem Rutsch.
fn lerp(a: u32, b: u32, t: u32) -> u32 {
    let rb = ((a & 0x00FF_00FF) * (256 - t) + (b & 0x00FF_00FF) * t) >> 8 & 0x00FF_00FF;
    let g = ((a & 0x0000_FF00) * (256 - t) + (b & 0x0000_FF00) * t) >> 8 & 0x0000_FF00;
    rb | g
}

/// Bilineare Skalierung des Frames in das Rechteck (ox, oy, dw, dh) des Zielpuffers.
fn scale_bilinear(
    f: &Frame,
    buffer: &mut [u32],
    bw: usize,
    ox: usize,
    oy: usize,
    dw: usize,
    dh: usize,
) {
    // Pro Zielspalte: linker Quellpixel und Gewicht — nur einmal berechnet.
    let xs: Vec<(usize, usize, u32)> = (0..dw)
        .map(|x| {
            let pos = ((x as u64 * 2 + 1) * f.width as u64 * 128 / dw as u64).saturating_sub(128); // Pixelmitte, 24.8 Fixpunkt
            let x0 = ((pos >> 8) as usize).min(f.width - 1);
            (x0, (x0 + 1).min(f.width - 1), (pos & 255) as u32)
        })
        .collect();
    for y in 0..dh {
        let pos = ((y as u64 * 2 + 1) * f.height as u64 * 128 / dh as u64).saturating_sub(128);
        let y0 = ((pos >> 8) as usize).min(f.height - 1);
        let y1 = (y0 + 1).min(f.height - 1);
        let ty = (pos & 255) as u32;
        let (r0, r1) = (
            &f.pixels[y0 * f.width..][..f.width],
            &f.pixels[y1 * f.width..][..f.width],
        );
        let row = &mut buffer[(oy + y) * bw + ox..][..dw];
        for (px, &(x0, x1, tx)) in row.iter_mut().zip(&xs) {
            let top = lerp(r0[x0], r0[x1], tx);
            let bottom = lerp(r1[x0], r1[x1], tx);
            *px = lerp(top, bottom, ty);
        }
    }
}
