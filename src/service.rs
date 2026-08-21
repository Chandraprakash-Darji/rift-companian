//! `service` subcommand: install and manage the user LaunchAgent that keeps
//! the indicator running in the background (no terminal required).
//!
//! Modeled on Rift's own launchd service handling.

use std::env;
use std::fs::{self, File};
use std::io::{self, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const LAUNCHCTL_PATH: &str = "/bin/launchctl";
const SERVICE_LABEL: &str = "com.rift.app-indicator";

pub fn run(command: Option<&str>) -> i32 {
    match command {
        Some("install") => match service_install() {
            Ok(msg) => {
                println!("{msg}");
                0
            }
            Err(e) => {
                eprintln!("Failed to install service: {e}");
                1
            }
        },
        Some("uninstall") => match service_uninstall() {
            Ok(msg) => {
                println!("{msg}");
                0
            }
            Err(e) => {
                eprintln!("Failed to uninstall service: {e}");
                1
            }
        },
        Some("start") => match service_start() {
            Ok(()) => {
                println!("Service started.");
                0
            }
            Err(e) => {
                eprintln!("Failed to start service: {e}");
                1
            }
        },
        Some("stop") => match service_stop() {
            Ok(()) => {
                println!("Service stopped.");
                0
            }
            Err(e) => {
                eprintln!("Failed to stop service: {e}");
                1
            }
        },
        Some("restart") => match service_restart() {
            Ok(()) => {
                println!("Service restarted.");
                0
            }
            Err(e) => {
                eprintln!("Failed to restart service: {e}");
                1
            }
        },
        Some(other) => {
            eprintln!("Unknown service command: {other}");
            print_usage();
            1
        }
        None => {
            print_usage();
            1
        }
    }
}

fn print_usage() {
    eprintln!(
        "Usage: rift-app-indicator service <install|uninstall|start|stop|restart>"
    );
}

fn user_home() -> io::Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "HOME not set"))
}

fn installed_bin_path() -> io::Result<PathBuf> {
    Ok(user_home()?.join(".local").join("bin").join("rift-app-indicator"))
}

fn plist_path() -> io::Result<PathBuf> {
    Ok(user_home()?
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{SERVICE_LABEL}.plist")))
}

fn service_target() -> io::Result<String> {
    let uid = unsafe { libc::getuid() };
    Ok(format!("gui/{uid}/{SERVICE_LABEL}"))
}

fn domain_target() -> io::Result<String> {
    let uid = unsafe { libc::getuid() };
    Ok(format!("gui/{uid}"))
}

fn install_executable_at(path: &Path) -> io::Result<()> {
    let current = env::current_exe()?;
    if current == path {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(&current, path).map(|_| ())
}

fn plist_contents() -> io::Result<String> {
    let user = env::var("USER").map_err(|_| io::Error::new(io::ErrorKind::Other, "env USER not set"))?;
    let path_env = env::var("PATH").map_err(|_| io::Error::new(io::ErrorKind::Other, "env PATH not set"))?;
    let exe = installed_bin_path()?;
    let exe_str = exe
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "non-UTF8 executable path"))?;

    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
    </array>
    <key>EnvironmentVariables</key>
    <dict>
        <key>PATH</key>
        <string>{path_env}</string>
    </dict>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
        <key>Crashed</key>
        <true/>
    </dict>
    <key>StandardOutPath</key>
    <string>/tmp/rift_app_indicator_{user}.out.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/rift_app_indicator_{user}.err.log</string>
    <key>ProcessType</key>
    <string>Interactive</string>
    <key>LimitLoadToSessionType</key>
    <string>Aqua</string>
</dict>
</plist>
"#,
        label = SERVICE_LABEL,
        exe = exe_str,
        path_env = path_env,
        user = user
    ))
}

fn ensure_parent_dir(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn write_file_atomic(path: &Path, contents: &str) -> io::Result<()> {
    ensure_parent_dir(path)?;
    let mut f = File::create(path)?;
    f.write_all(contents.as_bytes())?;
    Ok(())
}

fn run_launchctl(args: &[&str], suppress_output: bool) -> io::Result<i32> {
    let mut cmd = Command::new(LAUNCHCTL_PATH);
    cmd.args(args);
    if suppress_output {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let status = cmd.status()?;
    if let Some(code) = status.code() {
        Ok(code)
    } else {
        let sig = status.signal().unwrap_or_default();
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!("launchctl terminated by signal {sig}"),
        ))
    }
}

fn spawn_launchctl(args: &[&str]) -> io::Result<()> {
    let mut cmd = Command::new(LAUNCHCTL_PATH);
    cmd.args(args);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let _child = cmd.spawn()?;
    Ok(())
}

fn service_is_running() -> io::Result<bool> {
    match run_launchctl(&["print", &service_target()?], true) {
        Ok(code) => Ok(code == 0),
        Err(_) => Ok(false),
    }
}

fn write_plist_if_stale(plist_path: &Path) -> io::Result<bool> {
    let desired = plist_contents()?;
    match fs::read_to_string(plist_path) {
        Ok(existing) if existing == desired => Ok(false),
        Ok(_) => {
            write_file_atomic(plist_path, &desired)?;
            Ok(true)
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            write_file_atomic(plist_path, &desired)?;
            Ok(true)
        }
        Err(err) => Err(err),
    }
}

fn service_install() -> io::Result<&'static str> {
    let plist_path = plist_path()?;
    if plist_path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("service file '{}' is already installed", plist_path.display()),
        ));
    }
    install_executable_at(&installed_bin_path()?)?;
    write_plist_if_stale(&plist_path)?;
    Ok("Service installed. Start it with `rift-app-indicator service start`.")
}

fn service_uninstall() -> io::Result<&'static str> {
    let plist_path = plist_path()?;
    if !plist_path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("service file '{}' is not installed", plist_path.display()),
        ));
    }
    if service_is_running()? {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "service is still running; stop it first with `rift-app-indicator service stop`",
        ));
    }
    fs::remove_file(plist_path)?;
    Ok("Service uninstalled.")
}

fn service_start() -> io::Result<()> {
    let plist_path = plist_path()?;
    install_executable_at(&installed_bin_path()?)?;
    let plist_changed = write_plist_if_stale(&plist_path)?;

    let service_target = service_target()?;
    let domain_target = domain_target()?;

    let is_bootstrapped = run_launchctl(&["print", &service_target], true).unwrap_or(1);
    if is_bootstrapped != 0 {
        let _ = run_launchctl(&["enable", &service_target], true);
        let _ = spawn_launchctl(&["bootstrap", &domain_target, plist_path.to_str().unwrap()]);
        std::thread::sleep(Duration::from_millis(150));
        let code = run_launchctl(&["kickstart", &service_target], false)?;
        if code == 0 {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                format!("kickstart after bootstrap failed (exit {code})"),
            ))
        }
    } else {
        let code = run_launchctl(&["kickstart", &service_target], false)?;
        if code == 0 {
            return Ok(());
        }
        if plist_changed {
            let _ = run_launchctl(&["bootout", &domain_target, plist_path.to_str().unwrap()], true);
            let _ = spawn_launchctl(&["bootstrap", &domain_target, plist_path.to_str().unwrap()]);
            std::thread::sleep(Duration::from_millis(150));
            let code2 = run_launchctl(&["kickstart", &service_target], false)?;
            if code2 == 0 {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!(
                        "kickstart failed (exit {code}), reload+kickstart failed (exit {code2})"
                    ),
                ))
            }
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                format!("kickstart failed (exit {code})"),
            ))
        }
    }
}

fn service_restart() -> io::Result<()> {
    let plist_path = plist_path()?;
    if !plist_path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("service file '{}' is not installed", plist_path.display()),
        ));
    }
    let code = run_launchctl(&["kickstart", "-k", &service_target()?], false)?;
    if code == 0 {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!("kickstart -k failed (exit {code})"),
        ))
    }
}

fn service_stop() -> io::Result<()> {
    let plist_path = plist_path()?;
    if !plist_path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("service file '{}' is not installed", plist_path.display()),
        ));
    }

    let service_target = service_target()?;
    let domain_target = domain_target()?;

    let is_bootstrapped = run_launchctl(&["print", &service_target], true).unwrap_or(1);
    if is_bootstrapped == 0 {
        let code = run_launchctl(&["kill", "SIGTERM", &service_target], false)?;
        if code == 0 {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                format!("kill SIGTERM failed (exit {code})"),
            ))
        }
    } else {
        let code1 =
            run_launchctl(&["bootout", &domain_target, plist_path.to_str().unwrap()], false)?;
        let code2 = run_launchctl(&["disable", &service_target], false)?;
        if code1 == 0 && code2 == 0 {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "bootout failed (exit {code1}), disable failed (exit {code2})"
                ),
            ))
        }
    }
}