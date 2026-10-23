// Interface (egui): a panel with status, settings and log on the left, the iPhone picture in the middle.
// As soon as an iPhone mirrors, the panel is hidden and the picture fills the window.
use crate::dacp::{self, Command};
use crate::decoder::{Latest, Update};
use crate::settings::Settings;
use crate::shared::{self, Decision};
use crate::spectrum;
use crate::tray::{self, Tray, TrayAction};
use crate::{config, platform};
use eframe::egui::{
    self, Align2, Color32, ColorImage, Key, Panel, RichText, TextureHandle, TextureOptions,
    ViewportCommand, WindowLevel,
};
use std::time::{Duration, Instant};

const ACCENT: Color32 = Color32::from_rgb(64, 156, 255);
const OVERLAY_SECONDS: f32 = 2.5;
const HOME_SIZE: [f32; 2] = [780.0, 640.0]; // window size without a picture: panel and hint fit side by side

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Status,
    Settings,
    Log,
}

pub struct App {
    latest: Latest,
    texture: Option<TextureHandle>,
    new_image: Option<ColorImage>,
    frame_size: (usize, usize),
    sized_for: (usize, usize),
    connected: bool,
    fullscreen: bool,
    auto_fullscreen: bool,
    show_panel: bool,
    tab: Tab,
    edit: Settings, // working copy of the settings, applied with "Save"
    notice: Option<String>,
    tray: Option<Tray>,
    last_motion: Instant,
    cover: Option<TextureHandle>,
    cover_gen: u64,
    bars: [f32; spectrum::BANDS],
}

/// Starts the interface. Blocks until the program ends.
pub fn run(latest: Latest, start_hidden: bool) -> eframe::Result {
    let settings = shared::settings();
    let icon = egui::IconData {
        rgba: crate::icon::rgba(64),
        width: 64,
        height: 64,
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("AirBridge")
        .with_inner_size(HOME_SIZE)
        .with_min_inner_size([640.0, 460.0])
        .with_icon(icon);
    if settings.always_on_top {
        viewport = viewport.with_always_on_top();
    }
    if start_hidden {
        viewport = viewport.with_visible(false);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "AirBridge",
        options,
        Box::new(move |cc| {
            shared::set_ctx(cc.egui_ctx.clone());
            style(&cc.egui_ctx);
            let tray = match tray::create(cc.egui_ctx.clone()) {
                Ok(t) => Some(t),
                Err(e) => {
                    shared::log(format!("Tray-Symbol nicht verfügbar: {e:#}"));
                    None
                }
            };
            Ok(Box::new(App {
                latest,
                texture: None,
                new_image: None,
                frame_size: (0, 0),
                sized_for: (0, 0),
                connected: false,
                fullscreen: false,
                auto_fullscreen: false,
                show_panel: true,
                tab: Tab::Status,
                edit: settings,
                notice: None,
                tray,
                last_motion: Instant::now(),
                cover: None,
                cover_gen: 0,
                bars: [0.0; spectrum::BANDS],
            }))
        }),
    )
}

fn style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.selection.bg_fill = ACCENT;
    visuals.hyperlink_color = ACCENT;
    visuals.window_corner_radius = 10.0.into();
    ctx.set_visuals(visuals);
    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
    });
}

impl eframe::App for App {
    // Also runs while the window is hidden in the tray: tray clicks and new pictures are handled here.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(tray) = &self.tray {
            while let Ok(action) = tray.rx.try_recv() {
                match action {
                    TrayAction::Open => show_window(ctx),
                    TrayAction::Quit => std::process::exit(0),
                }
            }
        }

        let update = self.latest.lock().unwrap().take();
        match update {
            Some(Update::Frame(f)) => {
                if !self.connected {
                    self.connected = true;
                    self.show_panel = false;
                    let settings = shared::settings();
                    if settings.start_fullscreen && !self.fullscreen {
                        self.set_fullscreen(ctx, true);
                        self.auto_fullscreen = true;
                    }
                }
                self.frame_size = (f.width, f.height);
                let pixels = f
                    .pixels
                    .iter()
                    .map(|&p| Color32::from_rgb((p >> 16) as u8, (p >> 8) as u8, p as u8))
                    .collect();
                self.new_image = Some(ColorImage::new([f.width, f.height], pixels));
                if self.frame_size != self.sized_for && !self.fullscreen {
                    self.fit_window(ctx);
                }
            }
            Some(Update::Cleared) => {
                self.connected = false;
                self.show_panel = true;
                self.sized_for = (0, 0);
                self.texture = None;
                if self.auto_fullscreen {
                    self.set_fullscreen(ctx, false);
                    self.auto_fullscreen = false;
                }
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(egui::vec2(
                    HOME_SIZE[0],
                    HOME_SIZE[1],
                )));
            }
            None => {}
        }

        // A connection request has to become visible even if the window sits in the tray.
        if shared::with(|s| s.pending.is_some()) {
            show_window(ctx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        if let Some(img) = self.new_image.take() {
            match &mut self.texture {
                Some(t) => t.set(img, TextureOptions::LINEAR),
                None => {
                    self.texture = Some(ctx.load_texture("video", img, TextureOptions::LINEAR));
                }
            }
        }

        self.update_cover(&ctx);

        let settings = shared::settings();
        if ctx.input(|i| i.viewport().close_requested())
            && settings.minimize_to_tray
            && self.tray.is_some()
        {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        }

        // Keyboard
        if ctx.input(|i| i.key_pressed(Key::F11)) {
            let on = !self.fullscreen;
            self.set_fullscreen(&ctx, on);
            self.auto_fullscreen = false;
        }
        if self.fullscreen && ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.set_fullscreen(&ctx, false);
            self.auto_fullscreen = false;
        }
        if ctx.input(|i| i.pointer.velocity().length() > 0.0) {
            self.last_motion = Instant::now();
        }

        if self.show_panel && !self.fullscreen {
            Panel::left("side")
                .default_size(330.0)
                .size_range(300.0..=420.0)
                .show(ui, |ui| self.side_panel(ui));
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::from_gray(12)))
            .show(ui, |ui| self.center(ui));

        self.overlay(&ctx);
        self.approval_dialog(&ctx);

        if self.connected {
            ctx.request_repaint_after(Duration::from_millis(500)); // so the buttons can fade out again
        } else if shared::audio_recent() {
            ctx.request_repaint_after(Duration::from_millis(33)); // animate spectrum and progress
        } else if self.show_panel {
            ctx.request_repaint_after(Duration::from_secs(1)); // show new log lines and status
        }
    }
}

fn show_window(ctx: &egui::Context) {
    ctx.send_viewport_cmd(ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(ViewportCommand::Focus);
}

impl App {
    fn set_fullscreen(&mut self, ctx: &egui::Context, on: bool) {
        self.fullscreen = on;
        ctx.send_viewport_cmd(ViewportCommand::Fullscreen(on));
    }

    /// Fits the window to the picture (1 picture pixel = 1 screen pixel, scaled down only if needed).
    fn fit_window(&mut self, ctx: &egui::Context) {
        let ppp = ctx.pixels_per_point();
        let (w, h) = (
            self.frame_size.0 as f32 / ppp,
            self.frame_size.1 as f32 / ppp,
        );
        let monitor = ctx.input(|i| i.viewport().monitor_size);
        let scale = monitor
            .map(|m| (m.x * 0.95 / w).min(m.y * 0.85 / h).min(1.0))
            .unwrap_or(1.0);
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(egui::vec2(w * scale, h * scale)));
        self.sized_for = self.frame_size;
    }

    fn center(&mut self, ui: &mut egui::Ui) {
        let rect = ui.available_rect_before_wrap();
        if let (true, Some(tex)) = (self.connected, &self.texture) {
            let size = tex.size_vec2();
            let scale = (rect.width() / size.x).min(rect.height() / size.y);
            let fitted = egui::Rect::from_center_size(rect.center(), size * scale);
            let response = ui.allocate_rect(rect, egui::Sense::click());
            ui.painter().image(
                tex.id(),
                fitted,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            if response.double_clicked() {
                let on = !self.fullscreen;
                self.set_fullscreen(ui.ctx(), on);
                self.auto_fullscreen = false;
            }
            return;
        }

        // Music instead of a picture: the "now playing" view.
        if shared::audio_recent() || shared::with(|s| s.music.has_track()) {
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                self.player(ui, rect)
            });
            return;
        }

        // No picture yet: hint on how to connect.
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(rect.height() * 0.32);
                ui.label(RichText::new("Warte auf iPhone …").size(26.0).strong());
                ui.add_space(14.0);
                ui.label("Auf dem iPhone:");
                ui.label(
                    RichText::new("Kontrollzentrum > Bildschirmsynchronisierung")
                        .color(Color32::LIGHT_GRAY),
                );
                let name = shared::with(|s| s.settings.name.clone());
                ui.label(RichText::new(format!("> {name}")).color(ACCENT).strong());
                if !self.show_panel {
                    ui.add_space(10.0);
                    if ui.button("Menü öffnen").clicked() {
                        self.show_panel = true;
                    }
                }
            });
        });
    }

    /// Small buttons in the top right while the picture runs and the mouse has moved.
    fn overlay(&mut self, ctx: &egui::Context) {
        if !self.connected || self.last_motion.elapsed().as_secs_f32() > OVERLAY_SECONDS {
            return;
        }
        egui::Area::new(egui::Id::NULL.with("overlay"))
            .anchor(Align2::RIGHT_TOP, [-10.0, 10.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let label = if self.fullscreen {
                        "Vollbild beenden"
                    } else {
                        "Vollbild"
                    };
                    if ui.button(label).clicked() {
                        let on = !self.fullscreen;
                        self.set_fullscreen(ctx, on);
                        self.auto_fullscreen = false;
                    }
                    if !self.fullscreen && ui.button("Menü").clicked() {
                        self.show_panel = !self.show_panel;
                    }
                });
            });
    }

    fn approval_dialog(&mut self, ctx: &egui::Context) {
        let pending = shared::with(|s| s.pending.as_ref().map(|p| (p.name.clone(), p.ip.clone())));
        let Some((name, ip)) = pending else { return };
        egui::Modal::new(egui::Id::NULL.with("approve")).show(ctx, |ui| {
            ui.set_width(330.0);
            ui.heading("Verbindung zulassen?");
            ui.label(format!(
                "„{name}“ ({ip}) möchte den Bildschirm auf diesen PC spiegeln."
            ));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Zulassen").clicked() {
                    shared::answer(Decision::Once);
                }
                if ui.button("Immer zulassen").clicked() {
                    shared::answer(Decision::Always);
                    self.edit = shared::settings();
                }
                if ui.button("Ablehnen").clicked() {
                    shared::answer(Decision::Deny);
                }
            });
        });
    }

    fn side_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("AirBridge").size(22.0).strong());
            ui.label(RichText::new(concat!("v", env!("CARGO_PKG_VERSION"))).weak());
        });
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Status, "Status");
            ui.selectable_value(&mut self.tab, Tab::Settings, "Einstellungen");
            ui.selectable_value(&mut self.tab, Tab::Log, "Log");
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| match self.tab {
                Tab::Status => self.status_tab(ui),
                Tab::Settings => self.settings_tab(ui),
                Tab::Log => log_tab(ui),
            });
    }

    fn status_tab(&mut self, ui: &mut egui::Ui) {
        let (device, device_id, ip, latency, net_error) = shared::with(|s| {
            (
                s.device.clone(),
                s.device_id.clone(),
                s.ip.clone(),
                s.latency_ms,
                s.net_error.clone(),
            )
        });
        let name = shared::with(|s| s.settings.name.clone());

        if let Some(e) = net_error {
            ui.colored_label(Color32::LIGHT_RED, format!("Netzwerkfehler: {e}"));
        }
        let music = shared::with(|s| s.music.clone());
        let listening = shared::audio_recent() || music.has_track();
        let (color, text) = if self.connected {
            (
                Color32::from_rgb(80, 200, 120),
                format!("Verbunden mit {}", device.as_deref().unwrap_or("iPhone")),
            )
        } else if listening {
            (
                Color32::from_rgb(80, 200, 120),
                format!("Musik von {}", device.as_deref().unwrap_or("iPhone")),
            )
        } else {
            (
                Color32::from_rgb(240, 180, 60),
                "Wartet auf ein iPhone".to_string(),
            )
        };
        ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            ui.painter().circle_filled(dot.center(), 5.5, color);
            ui.label(RichText::new(text).strong());
        });
        ui.add_space(6.0);

        egui::Grid::new("status")
            .num_columns(2)
            .spacing([14.0, 8.0])
            .show(ui, |ui| {
                row(ui, "Name am iPhone", &name);
                row(ui, "Adresse", &format!("{ip}"));
                row(ui, "Geräte-ID", &device_id);
                row(
                    ui,
                    "Ports",
                    &format!(
                        "{} (RTSP), {} (Video)",
                        config::RTSP_PORT,
                        config::MIRROR_PORT
                    ),
                );
                if !self.connected && music.has_track() {
                    let now = if music.artist.is_empty() {
                        music.title.clone()
                    } else {
                        format!("{} – {}", music.title, music.artist)
                    };
                    row(ui, "Läuft gerade", &now);
                }
                if self.connected {
                    row(
                        ui,
                        "Auflösung",
                        &format!("{} × {}", self.frame_size.0, self.frame_size.1),
                    );
                    row(ui, "Verzögerung", &format!("{latency:.0} ms"));
                }
            });
        ui.add_space(10.0);
        ui.separator();
        ui.label(RichText::new("So verbindest du:").strong());
        ui.label("1. iPhone und PC im selben WLAN/Netzwerk");
        ui.label("2. Kontrollzentrum öffnen");
        ui.label("3. Bildschirmsynchronisierung antippen");
        ui.label(format!("4. „{name}“ auswählen"));
        ui.add_space(6.0);
        ui.label(RichText::new("Tipp: F11 oder Doppelklick auf das Bild = Vollbild").weak());
    }

    fn settings_tab(&mut self, ui: &mut egui::Ui) {
        let saved = shared::settings();
        ui.label(RichText::new("Gerät").strong());
        ui.label("Name auf dem iPhone");
        ui.text_edit_singleline(&mut self.edit.name);
        ui.add_space(6.0);

        ui.label(RichText::new("Fenster").strong());
        ui.checkbox(&mut self.edit.always_on_top, "Immer im Vordergrund");
        ui.checkbox(
            &mut self.edit.start_fullscreen,
            "Beim Verbinden automatisch Vollbild",
        );
        ui.add_space(6.0);

        ui.label(RichText::new("System").strong());
        ui.checkbox(&mut self.edit.autostart, "Mit Windows starten (im Tray)");
        ui.checkbox(
            &mut self.edit.minimize_to_tray,
            "Beim Schließen im Tray weiterlaufen",
        );
        ui.add_space(6.0);

        ui.label(RichText::new("Sicherheit").strong());
        ui.checkbox(
            &mut self.edit.require_confirm,
            "Neue Verbindungen bestätigen",
        );
        ui.checkbox(
            &mut self.edit.trusted_only,
            "Nur vertraute Geräte (andere still ablehnen)",
        );
        ui.label(
            RichText::new("Ohne Bestätigung kann sich jedes Gerät im selben Netzwerk verbinden.")
                .weak()
                .small(),
        );
        if self.edit.trusted.is_empty() {
            ui.label(RichText::new("Keine vertrauten Geräte").weak());
        } else {
            ui.label("Vertraute Geräte:");
            let mut remove = None;
            for (i, t) in self.edit.trusted.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(&t.name);
                    if ui.small_button("Entfernen").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                self.edit.trusted.remove(i);
            }
        }
        ui.add_space(10.0);

        let changed = self.edit != saved;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(changed, egui::Button::new("Speichern"))
                .clicked()
            {
                self.save(ui.ctx(), &saved);
            }
            if ui
                .add_enabled(changed, egui::Button::new("Verwerfen"))
                .clicked()
            {
                self.edit = saved.clone();
            }
        });
        if self.edit.name.trim() != saved.name {
            ui.label(
                RichText::new("Der neue Name erscheint nach dem Speichern auf dem iPhone.")
                    .weak()
                    .small(),
            );
        }
        if let Some(n) = &self.notice {
            ui.colored_label(Color32::LIGHT_RED, n);
        }

        ui.add_space(12.0);
        ui.separator();
        ui.label(RichText::new("Werkzeuge").strong());
        if ui.button("Firewall-Regel anlegen").clicked() {
            if let Err(e) = platform::firewall(true) {
                self.notice = Some(format!("{e:#}"));
            }
        }
        if ui.button("Einstellungsordner öffnen").clicked() {
            if let Some(dir) = crate::settings::path().parent() {
                let _ = std::process::Command::new("explorer").arg(dir).spawn();
            }
        }
        ui.add_space(8.0);
        ui.hyperlink_to(
            "Projekt auf GitHub",
            "https://github.com/30jannik06/AirBridge",
        );
    }

    fn save(&mut self, ctx: &egui::Context, old: &Settings) {
        self.notice = None;
        self.edit.name = Settings::clean_name(&self.edit.name);
        if self.edit.autostart != old.autostart {
            if let Err(e) = platform::set_autostart(self.edit.autostart) {
                self.notice = Some(format!("{e:#}"));
                self.edit.autostart = old.autostart;
            }
        }
        if self.edit.always_on_top != old.always_on_top {
            let level = if self.edit.always_on_top {
                WindowLevel::AlwaysOnTop
            } else {
                WindowLevel::Normal
            };
            ctx.send_viewport_cmd(ViewportCommand::WindowLevel(level));
        }
        shared::apply_settings(self.edit.clone());
    }
}

fn row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(RichText::new(label).weak());
    ui.label(value);
    ui.end_row();
}

fn log_tab(ui: &mut egui::Ui) {
    let lines: Vec<String> = shared::with(|s| s.log.iter().cloned().collect());
    ui.horizontal(|ui| {
        if ui.button("Kopieren").clicked() {
            ui.ctx().copy_text(lines.join("\n"));
        }
        if ui.button("Leeren").clicked() {
            shared::with(|s| s.log.clear());
        }
    });
    egui::ScrollArea::vertical()
        .stick_to_bottom(true)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for line in &lines {
                ui.label(RichText::new(line).monospace().small());
            }
        });
}

// ---------------------------------------------------------------------------------------------------------------
// Music view ("now playing")
// ---------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Icon {
    Previous,
    Next,
    Play,
    Pause,
}

impl App {
    /// Reloads the cover when the iPhone sent a new one (or drops it when playback ends).
    fn update_cover(&mut self, ctx: &egui::Context) {
        let (generation, bytes) = shared::with(|s| (s.music.cover_gen, s.music.cover.clone()));
        if generation == self.cover_gen {
            return;
        }
        self.cover_gen = generation;
        self.cover = bytes
            .and_then(|b| decode_cover(&b))
            .map(|img| ctx.load_texture("cover", img, TextureOptions::LINEAR));
    }

    fn target_bars(&self) -> [f32; spectrum::BANDS] {
        #[cfg(feature = "audio")]
        {
            spectrum::bands(&crate::playback::scope())
        }
        #[cfg(not(feature = "audio"))]
        {
            [0.0; spectrum::BANDS]
        }
    }

    fn player(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let music = shared::with(|s| s.music.clone());
        let playing = shared::audio_recent();

        // Bars: fast up, slow down
        let target = if playing {
            self.target_bars()
        } else {
            [0.0; spectrum::BANDS]
        };
        for (bar, t) in self.bars.iter_mut().zip(target) {
            *bar += (t - *bar) * if t > *bar { 0.6 } else { 0.12 };
        }

        let width = (rect.width() - 48.0).clamp(180.0, 440.0);
        ui.vertical_centered(|ui| {
            ui.set_max_width(width);
            ui.add_space(22.0);

            // Cover
            let side = width.min(rect.height() * 0.38).max(120.0);
            match &self.cover {
                Some(tex) => {
                    ui.add(
                        egui::Image::from_texture(tex)
                            .fit_to_exact_size(egui::vec2(side, side))
                            .corner_radius(14.0),
                    );
                }
                None => {
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
                    ui.painter().rect_filled(r, 14.0, Color32::from_gray(30));
                    ui.painter().text(
                        r.center(),
                        Align2::CENTER_CENTER,
                        "Kein Cover",
                        egui::FontId::proportional(16.0),
                        Color32::from_gray(110),
                    );
                }
            }
            ui.add_space(12.0);

            // Title
            let title = if music.title.is_empty() {
                "Unbekannter Titel"
            } else {
                &music.title
            };
            ui.label(RichText::new(title).size(24.0).strong());
            if !music.artist.is_empty() {
                ui.label(RichText::new(&music.artist).size(16.0).color(ACCENT));
            }
            if !music.album.is_empty() {
                ui.label(RichText::new(&music.album).weak());
            }
            ui.add_space(10.0);

            // Spectrum
            let (area, _) = ui.allocate_exact_size(egui::vec2(width, 54.0), egui::Sense::hover());
            let slot = area.width() / spectrum::BANDS as f32;
            for (i, v) in self.bars.iter().enumerate() {
                let h = (v * area.height()).max(2.0);
                let x = area.left() + i as f32 * slot;
                let bar = egui::Rect::from_min_max(
                    egui::pos2(x + 1.5, area.bottom() - h),
                    egui::pos2(x + slot - 1.5, area.bottom()),
                );
                let alpha = (90.0 + 165.0 * v) as u8;
                ui.painter().rect_filled(
                    bar,
                    2.0,
                    Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), alpha),
                );
            }
            ui.add_space(6.0);

            // Progress
            if let Some((pos, dur)) = music.position_now(playing) {
                let (r, _) = ui.allocate_exact_size(egui::vec2(width, 30.0), egui::Sense::hover());
                let track =
                    egui::Rect::from_min_size(r.min + egui::vec2(0.0, 4.0), egui::vec2(width, 5.0));
                ui.painter().rect_filled(track, 3.0, Color32::from_gray(45));
                let frac = if dur > 0.0 { (pos / dur) as f32 } else { 0.0 };
                let filled = egui::Rect::from_min_size(
                    track.min,
                    egui::vec2(width * frac.clamp(0.0, 1.0), 5.0),
                );
                ui.painter().rect_filled(filled, 3.0, ACCENT);
                let font = egui::FontId::proportional(12.0);
                ui.painter().text(
                    r.left_bottom(),
                    Align2::LEFT_BOTTOM,
                    fmt_time(pos),
                    font.clone(),
                    Color32::GRAY,
                );
                ui.painter().text(
                    r.right_bottom(),
                    Align2::RIGHT_BOTTOM,
                    format!("-{}", fmt_time((dur - pos).max(0.0))),
                    font,
                    Color32::GRAY,
                );
            }
            ui.add_space(4.0);

            // Controls
            let remote = music.dacp.as_ref().is_some_and(|d| d.target.is_some());
            let mut command = None;
            ui.horizontal(|ui| {
                ui.add_space((width - 3.0 * 52.0 - 2.0 * 16.0).max(0.0) / 2.0);
                if icon_button(ui, Icon::Previous, remote) {
                    command = Some(Command::Previous);
                }
                let middle = if playing { Icon::Pause } else { Icon::Play };
                if icon_button(ui, middle, remote) {
                    command = Some(Command::PlayPause);
                }
                if icon_button(ui, Icon::Next, remote) {
                    command = Some(Command::Next);
                }
            });
            if let Some(cmd) = command {
                std::thread::spawn(move || {
                    if let Err(e) = dacp::send(cmd) {
                        log!("[Fernsteuerung] {e:#}");
                    }
                });
            }
            if !remote {
                ui.label(
                    RichText::new("Bedienung nur am iPhone (Fernsteuerung nicht verfügbar)")
                        .weak()
                        .small(),
                );
            }

            // Volume (controlled by the iPhone)
            ui.add_space(6.0);
            let pct = volume_percent(music.volume_db);
            ui.label(RichText::new(format!("Lautstärke {pct} %")).weak().small());
        });
    }
}

/// Same scale as on the iPhone: -30 dB = very quiet (0 %), 0 dB = full (100 %), -144 = mute.
fn volume_percent(db: f32) -> u32 {
    if db <= -100.0 {
        0
    } else {
        (((db + 30.0) / 30.0).clamp(0.0, 1.0) * 100.0).round() as u32
    }
}

fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

fn decode_cover(bytes: &[u8]) -> Option<ColorImage> {
    let img = image::load_from_memory(bytes)
        .ok()?
        .thumbnail(640, 640)
        .to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Some(ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
}

/// Round button with a self-drawn symbol (the built-in font has no media symbols).
fn icon_button(ui: &mut egui::Ui, icon: Icon, enabled: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(52.0, 52.0),
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    let hot = enabled && response.hovered();
    let bg = if hot {
        Color32::from_gray(60)
    } else {
        Color32::from_gray(36)
    };
    let fg = if enabled {
        Color32::WHITE
    } else {
        Color32::from_gray(90)
    };
    let p = ui.painter();
    p.circle_filled(rect.center(), 26.0, bg);
    let c = rect.center();
    let tri = |p: &egui::Painter, tip_x: f32, back_x: f32| {
        p.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(tip_x, c.y),
                egui::pos2(back_x, c.y - 9.0),
                egui::pos2(back_x, c.y + 9.0),
            ],
            fg,
            egui::Stroke::NONE,
        ));
    };
    let bar = |p: &egui::Painter, x: f32, w: f32, h: f32| {
        p.rect_filled(
            egui::Rect::from_center_size(egui::pos2(x, c.y), egui::vec2(w, h)),
            1.0,
            fg,
        );
    };
    match icon {
        Icon::Play => tri(p, c.x + 9.0, c.x - 7.0),
        Icon::Pause => {
            bar(p, c.x - 5.0, 5.0, 18.0);
            bar(p, c.x + 5.0, 5.0, 18.0);
        }
        Icon::Next => {
            tri(p, c.x + 4.0, c.x - 8.0);
            bar(p, c.x + 8.0, 3.0, 18.0);
        }
        Icon::Previous => {
            tri(p, c.x - 4.0, c.x + 8.0);
            bar(p, c.x - 8.0, 3.0, 18.0);
        }
    }
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_format() {
        assert_eq!(fmt_time(0.0), "0:00");
        assert_eq!(fmt_time(75.9), "1:15");
        assert_eq!(fmt_time(-3.0), "0:00");
    }

    #[test]
    fn volume_scale() {
        assert_eq!(volume_percent(0.0), 100);
        assert_eq!(volume_percent(-30.0), 0);
        assert_eq!(volume_percent(-15.0), 50);
        assert_eq!(volume_percent(-144.0), 0);
    }

    #[test]
    fn bad_cover_data_is_ignored() {
        assert!(decode_cover(b"das ist kein Bild").is_none());
        assert!(decode_cover(&[]).is_none());
    }
}
