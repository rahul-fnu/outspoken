use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ActiveAppInfo {
    pub name: String,
    pub title: String,
    pub process: String,
}

/// Returns information about the currently active (frontmost) application.
pub fn get_active_app() -> Result<ActiveAppInfo, String> {
    platform::get_active_app()
}

#[cfg(target_os = "macos")]
mod platform {
    use super::ActiveAppInfo;
    use std::process::Command;

    pub fn get_active_app() -> Result<ActiveAppInfo, String> {
        // Use AppleScript to get frontmost application info
        let script = r#"
            tell application "System Events"
                set frontApp to first application process whose frontmost is true
                set appName to name of frontApp
                set winTitle to ""
                try
                    set winTitle to name of front window of frontApp
                end try
                set appBundle to bundle identifier of frontApp
                return appName & "\n" & winTitle & "\n" & appBundle
            end tell
        "#;

        let output = Command::new("osascript")
            .arg("-e")
            .arg(script)
            .output()
            .map_err(|e| format!("Failed to run osascript: {e}"))?;

        if !output.status.success() {
            return Err(format!(
                "osascript failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let parts: Vec<&str> = stdout.trim().splitn(3, '\n').collect();

        Ok(ActiveAppInfo {
            name: parts.first().unwrap_or(&"").to_string(),
            title: parts.get(1).unwrap_or(&"").to_string(),
            process: parts.get(2).unwrap_or(&"").to_string(),
        })
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::ActiveAppInfo;
    use std::process::Command;

    pub fn get_active_app() -> Result<ActiveAppInfo, String> {
        // Use PowerShell to get the foreground window info
        let script = r#"
            Add-Type @"
                using System;
                using System.Runtime.InteropServices;
                using System.Text;
                public class WinAPI {
                    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
                    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int count);
                    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
                }
"@
            $hwnd = [WinAPI]::GetForegroundWindow()
            $sb = New-Object System.Text.StringBuilder 256
            [void][WinAPI]::GetWindowText($hwnd, $sb, 256)
            $title = $sb.ToString()
            $pid = 0
            [void][WinAPI]::GetWindowThreadProcessId($hwnd, [ref]$pid)
            $proc = Get-Process -Id $pid -ErrorAction SilentlyContinue
            $name = if ($proc) { $proc.ProcessName } else { "" }
            Write-Output "$name`n$title`n$pid"
        "#;

        let output = Command::new("powershell")
            .args(["-NoProfile", "-Command", script])
            .output()
            .map_err(|e| format!("Failed to run powershell: {e}"))?;

        if !output.status.success() {
            return Err(format!(
                "PowerShell failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let parts: Vec<&str> = stdout.trim().splitn(3, '\n').collect();

        Ok(ActiveAppInfo {
            name: parts.first().unwrap_or(&"").to_string(),
            title: parts.get(1).unwrap_or(&"").to_string(),
            process: parts.get(2).unwrap_or(&"").to_string(),
        })
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::ActiveAppInfo;
    use std::process::Command;

    pub fn get_active_app() -> Result<ActiveAppInfo, String> {
        // Try xdotool first (works on X11)
        let window_id = Command::new("xdotool")
            .arg("getactivewindow")
            .output()
            .map_err(|e| format!("Failed to run xdotool: {e}"))?;

        if !window_id.status.success() {
            return Err("No active window found (xdotool failed). Wayland may not be supported.".into());
        }

        let wid = String::from_utf8_lossy(&window_id.stdout).trim().to_string();

        // Get window title
        let title_output = Command::new("xdotool")
            .args(["getwindowname", &wid])
            .output()
            .ok();
        let title = title_output
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();

        // Get PID
        let pid_output = Command::new("xdotool")
            .args(["getwindowpid", &wid])
            .output()
            .ok();
        let pid = pid_output
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();

        // Get process name from /proc/{pid}/comm
        let name = if !pid.is_empty() {
            std::fs::read_to_string(format!("/proc/{pid}/comm"))
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        } else {
            String::new()
        };

        Ok(ActiveAppInfo {
            name,
            title,
            process: pid,
        })
    }
}
