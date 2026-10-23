// Oberfläche (egui): links ein Panel mit Status, Einstellungen und Log, in der Mitte das iPhone-Bild.
// Sobald ein iPhone spiegelt, wird das Panel ausgeblendet und das Bild füllt das Fenster.
use crate::decoder::{Latest, Update};
use crate::settings::Settings;
use crate::shared::{self, Decision};
use crate::tray::{self, Tray, TrayAction};
use crate::{config, platform};
use eframe::egui::{
    self, Align2, Color32, ColorImage, Key, Panel, RichText, TextureHandle, TextureOptions,
    ViewportCommand, WindowLevel,
};
use std::time::{Duration, Instant};

const ACCENT: Color32 = Color32::from_rgb(64, 156, 255);
const OVERLAY_SECONDS: f32 = 2.5;
const HOME_SIZE: [f32; 2] = [780.0, 640.0]; // Fenstergröße ohne Bild: Panel + Hinweis passen nebeneinander

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
    edit: Settings, // Arbeitskopie der Einstellungen, wird mit "Speichern" übernommen
    notice: Option<String>,
    tray: Option<Tray>,
    last_motion: Instant,
}

/// Startet die Oberfläche. Blockiert, bis das Programm beendet wird.
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
    // Läuft auch, wenn das Fenster im Tray versteckt ist: hier werden Tray-Klicks und neue Bilder verarbeitet.
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

        // Eine Verbindungsanfrage muss sichtbar werden, auch wenn das Fenster im Tray steckt.
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

        let settings = shared::settings();
        if ctx.input(|i| i.viewport().close_requested())
            && settings.minimize_to_tray
            && self.tray.is_some()
        {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        }

        // Tastatur
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
            ctx.request_repaint_after(Duration::from_millis(500)); // damit die Schaltflächen wieder verschwinden
        } else if self.show_panel {
            ctx.request_repaint_after(Duration::from_secs(1)); // neue Log-Zeilen und Status anzeigen
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

    /// Passt die Fenstergröße an das Bild an (1 Bildpixel = 1 Bildschirmpixel, nur bei Bedarf verkleinert).
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

        // Noch kein Bild: Hinweis, wie man verbindet.
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

    /// Kleine Schaltflächen oben rechts, solange das Bild läuft und die Maus sich bewegt hat.
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
        let (color, text) = if self.connected {
            (
                Color32::from_rgb(80, 200, 120),
                format!("Verbunden mit {}", device.as_deref().unwrap_or("iPhone")),
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
