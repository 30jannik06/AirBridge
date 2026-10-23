// Windows-spezifische Helfer: Konsole ausblenden, Dialoge, Firewall-Regel.
use anyhow::{Result, bail};
use std::os::windows::process::CommandExt;
use std::process::Command;
use windows::Win32::System::Console::{FreeConsole, GetConsoleProcessList};
use windows::Win32::UI::Shell::{IsUserAnAdmin, ShellExecuteW};
use windows::Win32::UI::WindowsAndMessaging::{
    MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MessageBoxW, SW_SHOWNORMAL,
};
use windows::core::{HSTRING, w};

const RULE_NAME: &str = "AirBridge";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Doppelklick-Start: Das Programm ist allein an der Konsole -> Fenster schließen.
/// Start aus PowerShell/cmd (oder cargo): Die Konsole gehört auch anderen -> bleibt, damit man das Log sieht.
pub fn hide_console_if_alone() {
    let mut ids = [0u32; 4];
    if unsafe { GetConsoleProcessList(&mut ids) } == 1 {
        let _ = unsafe { FreeConsole() };
    }
}

/// Zeigt einen Dialog. Nötig, weil Fehler sonst unsichtbar wären, wenn die Konsole ausgeblendet ist.
pub fn message(title: &str, text: &str, error: bool) {
    let icon = if error {
        MB_ICONERROR
    } else {
        MB_ICONINFORMATION
    };
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(text),
            &HSTRING::from(title),
            MB_OK | icon,
        )
    };
}

/// Legt die Firewall-Regel an (`install = true`) oder entfernt sie. Braucht Administratorrechte:
/// Ohne diese startet sich das Programm selbst neu (Windows fragt dann per UAC nach der Erlaubnis).
pub fn firewall(install: bool) -> Result<()> {
    let exe = std::env::current_exe()?.to_string_lossy().to_string();
    let flag = if install {
        "--install-firewall"
    } else {
        "--remove-firewall"
    };

    if !unsafe { IsUserAnAdmin() }.as_bool() {
        let result = unsafe {
            ShellExecuteW(
                None,
                w!("runas"),
                &HSTRING::from(exe.as_str()),
                &HSTRING::from(flag),
                None,
                SW_SHOWNORMAL,
            )
        };
        if result.0 as usize <= 32 {
            bail!("Administratorrechte wurden nicht erteilt");
        }
        return Ok(()); // der neue, erhöhte Prozess erledigt den Rest und zeigt das Ergebnis an
    }

    let netsh = |args: &[String]| -> Result<(bool, String)> {
        let out = Command::new("netsh")
            .args(["advfirewall", "firewall"])
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .output()?;
        Ok((
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).trim().to_string(),
        ))
    };
    let delete = vec!["delete".into(), "rule".into(), format!("name={RULE_NAME}")];
    let (deleted, _) = netsh(&delete)?; // alte Regel entfernen, damit nichts doppelt existiert

    if !install {
        message(
            "AirBridge",
            if deleted {
                "Firewall-Regel wurde entfernt."
            } else {
                "Es gab keine Firewall-Regel."
            },
            false,
        );
        return Ok(());
    }
    let add = vec![
        "add".into(),
        "rule".into(),
        format!("name={RULE_NAME}"),
        "dir=in".into(),
        "action=allow".into(),
        format!("program={exe}"),
        "enable=yes".into(),
        "profile=private,domain".into(),
    ];
    let (ok, output) = netsh(&add)?;
    if ok {
        message(
            "AirBridge",
            &format!(
                "Firewall-Regel angelegt.\n\nAirBridge darf jetzt in privaten Netzwerken Verbindungen annehmen:\n{exe}"
            ),
            false,
        );
        Ok(())
    } else {
        bail!("netsh meldet: {output}")
    }
}
