// Windows-specific helpers: hide the console, dialogs, firewall rule.
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

/// Started by double click: the program is alone on the console -> close the window.
/// Started from PowerShell/cmd (or cargo): the console also belongs to others -> it stays, so you can see the log.
pub fn hide_console_if_alone() {
    let mut ids = [0u32; 4];
    if unsafe { GetConsoleProcessList(&mut ids) } == 1 {
        let _ = unsafe { FreeConsole() };
    }
}

/// Shows a dialog. Needed because errors would otherwise be invisible when the console is hidden.
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

/// `true` if another AirBridge is already running. The named mutex also lets the installer detect a running copy.
pub fn already_running() -> bool {
    use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows::Win32::System::Threading::CreateMutexW;
    let handle = unsafe { CreateMutexW(None, false, w!("AirBridgeSingleInstance")) };
    let exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    // Keep the handle open for the whole program run (it is closed when the process ends).
    std::mem::forget(handle);
    exists
}

/// Opens a web page in the default browser.
pub fn open_url(url: &str) {
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(url),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

/// Turns "start with Windows" on or off (entry in the current user's Run key, no admin rights needed).
/// It starts with `--minimized`: AirBridge then only sits in the tray.
pub fn set_autostart(on: bool) -> Result<()> {
    let reg = |args: &[String]| -> Result<bool> {
        Ok(Command::new("reg")
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .output()?
            .status
            .success())
    };
    let ok = if on {
        let exe = std::env::current_exe()?;
        reg(&[
            "add".into(),
            RUN_KEY.into(),
            "/v".into(),
            RULE_NAME.into(),
            "/t".into(),
            "REG_SZ".into(),
            "/d".into(),
            format!("\"{}\" --minimized", exe.display()),
            "/f".into(),
        ])?
    } else {
        // If the entry is already gone, that is fine too.
        let _ = reg(&[
            "delete".into(),
            RUN_KEY.into(),
            "/v".into(),
            RULE_NAME.into(),
            "/f".into(),
        ])?;
        true
    };
    if ok {
        Ok(())
    } else {
        bail!("Der Autostart-Eintrag konnte nicht geschrieben werden")
    }
}

/// Creates the firewall rule (`install = true`) or removes it. Needs administrator rights:
/// without them the program restarts itself (Windows then asks for permission via UAC).
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
        return Ok(()); // the new, elevated process does the rest and shows the result
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
    let (deleted, _) = netsh(&delete)?; // remove the old rule so nothing exists twice

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
