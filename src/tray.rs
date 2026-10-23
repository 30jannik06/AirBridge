// Icon in the notification area of the taskbar (tray): left click opens the window, right click shows a small menu.
// The events come from the Windows message loop; we pass them on to the interface through a channel.
use anyhow::Result;
use eframe::egui;
use std::sync::mpsc::{Receiver, channel};
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub enum TrayAction {
    Open,
    Quit,
}

pub struct Tray {
    _icon: TrayIcon, // must stay alive, otherwise the icon disappears
    pub rx: Receiver<TrayAction>,
}

pub fn create(ctx: egui::Context) -> Result<Tray> {
    let menu = Menu::new();
    let open = MenuItem::new("AirBridge öffnen", true, None);
    let quit = MenuItem::new("Beenden", true, None);
    menu.append(&open)?;
    menu.append(&quit)?;
    let (open_id, quit_id) = (open.id().clone(), quit.id().clone());

    let (tx, rx) = channel();
    {
        let (tx, ctx) = (tx.clone(), ctx.clone());
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let action = if e.id == open_id {
                TrayAction::Open
            } else if e.id == quit_id {
                TrayAction::Quit
            } else {
                return;
            };
            let _ = tx.send(action);
            ctx.request_repaint();
        }));
    }
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = e
        {
            let _ = tx.send(TrayAction::Open);
            ctx.request_repaint();
        }
    }));

    let icon = Icon::from_rgba(crate::icon::rgba(32), 32, 32)?;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip("AirBridge")
        .with_icon(icon)
        .build()?;
    Ok(Tray { _icon: tray, rx })
}
