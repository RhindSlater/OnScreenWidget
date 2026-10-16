//! Click actions: open URLs, launch terminals, SSH, run remote commands.
//!
//! Terminal launching is OS-specific. On Windows we prefer Windows Terminal
//! (`wt.exe`) and fall back to `cmd`. On macOS we use `Terminal.app` via
//! `open`/`osascript`.

use crate::model::ActionSpec;
use crate::secrets;
use std::process::Command;
use std::time::Duration;

/// Perform an HTTP-request action asynchronously. Reads the auth token from the
/// OS keychain (if configured) and sends it in the given header. Returns an
/// error if the request fails or the server responds with a non-2xx status.
pub async fn run_http_request(
    method: &str,
    url: &str,
    auth_header: Option<&str>,
    auth_token_secret_ref: Option<&str>,
    body: Option<&str>,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("OnScreenWidget/0.1")
        .build()
        .map_err(|e| format!("build client: {e}"))?;

    let m = reqwest::Method::from_bytes(method.to_uppercase().as_bytes())
        .map_err(|_| format!("invalid HTTP method: {method}"))?;
    let mut req = client.request(m, url);

    // Attach the auth token from the keychain, if configured.
    if let (Some(header), Some(secret_ref)) = (auth_header, auth_token_secret_ref) {
        if !header.is_empty() && !secret_ref.is_empty() {
            match secrets::get_secret(secret_ref) {
                Ok(Some(token)) => {
                    req = req.header(header, token);
                }
                Ok(None) => {
                    return Err(format!("auth token '{secret_ref}' not found in keychain"))
                }
                Err(e) => return Err(format!("read token: {e}")),
            }
        }
    }

    if let Some(b) = body {
        req = req.body(b.to_string());
    }

    let resp = req.send().await.map_err(|e| format!("request failed: {e}"))?;
    let status = resp.status();
    if status.is_success() {
        Ok(())
    } else {
        let txt = resp.text().await.unwrap_or_default();
        Err(format!("HTTP {} • {}", status.as_u16(), txt.chars().take(120).collect::<String>()))
    }
}

/// Execute a backend-handled action. `Frontend` actions are a no-op here (the
/// UI handles them) and return Ok.
pub fn run_action(action: &ActionSpec) -> Result<(), String> {
    match action {
        ActionSpec::OpenUrl { url } => open_url(url),
        ActionSpec::SshTerminal { host, user, port } => {
            open_terminal_ssh(host, user.as_deref(), *port)
        }
        ActionSpec::SshCommand {
            host,
            user,
            port,
            command,
        } => run_ssh_command(host, user.as_deref(), *port, command),
        ActionSpec::SpawnTerminal { program, args } => spawn_terminal(program, args),
        ActionSpec::LaunchApp { path, args } => launch_app(path, args),
        ActionSpec::HttpRequest { .. } => {
            // Handled asynchronously in lib.rs via run_http_request(); this sync
            // path should not be used for it.
            Err("http_request must be run via the async handler".into())
        }
        ActionSpec::Frontend { .. } => Ok(()),
    }
}

fn target(host: &str, user: Option<&str>) -> String {
    match user {
        Some(u) => format!("{u}@{host}"),
        None => host.to_string(),
    }
}

/// Open the default browser.
fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map_err(|e| format!("open url: {e}"))?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("open url: {e}"))?;
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("open url: {e}"))?;
    }
    Ok(())
}

/// Launch an application/executable/shortcut, with no terminal window.
///
/// Windows `.lnk` shortcuts (and `.url`, registered file types) are resolved by
/// the shell, not by `CreateProcess`, so a plain `Command::new(path)` can't
/// launch them. We therefore go through the OS shell:
///   - Windows: PowerShell `Start-Process` (handles .exe, .lnk, .url, docs).
///   - macOS:   `open` (optionally `open -a` for .app bundles).
///   - Linux:   run the binary directly, falling back to `xdg-open`.
fn launch_app(path: &str, args: &[String]) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        // Build: Start-Process -FilePath '<path>' [-ArgumentList '<a>','<b>']
        // Single-quote and escape for PowerShell (doubling embedded quotes).
        let ps_quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
        let mut cmd = format!("Start-Process -FilePath {}", ps_quote(path));
        if !args.is_empty() {
            let list = args
                .iter()
                .map(|a| ps_quote(a))
                .collect::<Vec<_>>()
                .join(",");
            cmd.push_str(&format!(" -ArgumentList {list}"));
        }
        Command::new("powershell")
            .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &cmd])
            .spawn()
            .map_err(|e| format!("launch app '{path}': {e}"))?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        // `.app` bundles are opened with -a; everything else opened directly.
        let mut c = Command::new("open");
        if path.ends_with(".app") || path.ends_with(".app/") {
            c.arg("-a").arg(path);
            if !args.is_empty() {
                c.arg("--args").args(args);
            }
        } else {
            c.arg(path);
            if !args.is_empty() {
                c.arg("--args").args(args);
            }
        }
        c.spawn().map_err(|e| format!("launch app '{path}': {e}"))?;
        return Ok(());
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        // Try to execute directly; if that fails, let the desktop open it.
        match Command::new(path).args(args).spawn() {
            Ok(_) => Ok(()),
            Err(_) => {
                Command::new("xdg-open")
                    .arg(path)
                    .spawn()
                    .map_err(|e| format!("launch app '{path}': {e}"))?;
                Ok(())
            }
        }
    }
}

/// Open an interactive terminal and SSH into the target.
fn open_terminal_ssh(host: &str, user: Option<&str>, port: u16) -> Result<(), String> {
    let tgt = target(host, user);
    let ssh_args = vec!["-p".to_string(), port.to_string(), tgt];
    spawn_terminal("ssh", &ssh_args)
}

/// Run a non-interactive command over SSH (used for restart etc.).
/// Relies on key-based auth; captures output for error reporting.
fn run_ssh_command(host: &str, user: Option<&str>, port: u16, command: &str) -> Result<(), String> {
    let tgt = target(host, user);
    let output = Command::new("ssh")
        .args([
            "-p",
            &port.to_string(),
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            &tgt,
            command,
        ])
        .output()
        .map_err(|e| format!("spawn ssh: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&output.stderr);
        Err(format!("ssh command failed: {}", err.trim()))
    }
}

/// Open a terminal running `program` with `args`.
#[cfg(target_os = "windows")]
fn spawn_terminal(program: &str, args: &[String]) -> Result<(), String> {
    // Try Windows Terminal first; fall back to cmd.
    let joined = shell_join(program, args);
    let wt = Command::new("wt.exe")
        .args(["new-tab", "cmd", "/K", &joined])
        .spawn();
    if wt.is_ok() {
        return Ok(());
    }
    Command::new("cmd")
        .args(["/C", "start", "cmd", "/K", &joined])
        .spawn()
        .map_err(|e| format!("spawn terminal: {e}"))?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn spawn_terminal(program: &str, args: &[String]) -> Result<(), String> {
    let joined = shell_join(program, args);
    // Open Terminal.app and run the command via AppleScript.
    let script = format!(
        "tell application \"Terminal\" to do script \"{}\"",
        joined.replace('"', "\\\"")
    );
    Command::new("osascript")
        .args(["-e", &script])
        .spawn()
        .map_err(|e| format!("spawn terminal: {e}"))?;
    Ok(())
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn spawn_terminal(program: &str, args: &[String]) -> Result<(), String> {
    let joined = shell_join(program, args);
    // Best-effort on Linux: try a few common terminals.
    for term in ["x-terminal-emulator", "gnome-terminal", "konsole", "xterm"] {
        if Command::new(term)
            .args(["-e", "bash", "-lc", &format!("{joined}; exec bash")])
            .spawn()
            .is_ok()
        {
            return Ok(());
        }
    }
    Err("no terminal emulator found".into())
}

/// Join a program and args into a single command line, quoting args with spaces.
fn shell_join(program: &str, args: &[String]) -> String {
    let mut parts = vec![quote(program)];
    for a in args {
        parts.push(quote(a));
    }
    parts.join(" ")
}

fn quote(s: &str) -> String {
    if s.contains(' ') || s.contains('"') {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_target_with_user() {
        assert_eq!(target("host", Some("root")), "root@host");
    }

    #[test]
    fn builds_target_without_user() {
        assert_eq!(target("host", None), "host");
    }

    #[test]
    fn quotes_args_with_spaces() {
        assert_eq!(shell_join("ssh", &["my host".to_string()]), "ssh \"my host\"");
    }

    #[test]
    fn leaves_simple_args_unquoted() {
        assert_eq!(
            shell_join("ssh", &["-p".to_string(), "22".to_string(), "root@h".to_string()]),
            "ssh -p 22 root@h"
        );
    }
}
