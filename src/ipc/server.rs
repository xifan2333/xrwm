use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use rustix::event::{PollFd, PollFlags, Timespec};

pub(crate) const MAX_SUN_LEN: usize = 107; // 108 bytes minus null terminator

fn fnv1a_64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn is_safe_relative_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 32 {
        return false;
    }
    if name.starts_with('.') || name.starts_with('-') {
        return false;
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

pub fn format_socket_name(display: &str) -> String {
    let display_str = if display.trim().is_empty() {
        "wayland-0"
    } else {
        display
    };

    if is_safe_relative_name(display_str) {
        format!("xrwm-{display_str}.sock")
    } else {
        let hash = fnv1a_64(display_str.as_bytes());
        let raw_basename = Path::new(display_str)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        let sanitized: String = raw_basename
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .take(16)
            .collect();

        let prefix = if sanitized.is_empty() {
            "display"
        } else {
            &sanitized
        };

        format!("xrwm-{prefix}-{hash:016x}.sock")
    }
}

pub fn socket_path_for_display(xdg_runtime_dir: &Path, display: &str) -> PathBuf {
    let socket_name = format_socket_name(display);
    let path = xdg_runtime_dir.join(&socket_name);
    if path.as_os_str().len() > MAX_SUN_LEN {
        let uid = rustix::process::getuid().as_raw();
        let xdg_hash = fnv1a_64(xdg_runtime_dir.as_os_str().as_encoded_bytes());
        let fallback_dir = PathBuf::from(format!("/tmp/xrwm-{uid}"));
        let fallback_socket_name = format!("{xdg_hash:08x}-{socket_name}");
        fallback_dir.join(fallback_socket_name)
    } else {
        path
    }
}

pub fn get_socket_path() -> PathBuf {
    let xdg = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_string());
    socket_path_for_display(Path::new(&xdg), &display)
}

pub fn create_ipc_server_at(socket_path: &std::path::Path) -> std::io::Result<UnixListener> {
    if let Some(parent) = socket_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                std::fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(parent)?;
            }
            #[cfg(not(unix))]
            {
                std::fs::create_dir_all(parent)?;
            }
        } else if parent.exists() {
            #[cfg(unix)]
            if parent.starts_with("/tmp/xrwm-") {
                use std::os::unix::fs::MetadataExt;
                let meta = std::fs::metadata(parent)?;
                let current_uid = rustix::process::getuid().as_raw();
                if meta.uid() != current_uid {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        format!(
                            "Fallback directory {parent:?} is owned by UID {}, expected UID {current_uid}",
                            meta.uid()
                        ),
                    ));
                }
                if (meta.mode() & 0o077) != 0 {
                    use std::os::unix::fs::PermissionsExt;
                    let mut perms = meta.permissions();
                    perms.set_mode(0o700);
                    let _ = std::fs::set_permissions(parent, perms);
                }
            }
        }
    }

    if socket_path.exists() {
        match UnixStream::connect(socket_path) {
            Ok(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AddrInUse,
                    format!("Another xrwm instance is already listening on {socket_path:?}"),
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                let _ = std::fs::remove_file(socket_path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(e);
            }
        }
    }
    UnixListener::bind(socket_path)
}

pub fn create_ipc_server() -> std::io::Result<UnixListener> {
    let socket_path = get_socket_path();
    create_ipc_server_at(&socket_path)
}

pub struct IpcServerGuard {
    path: PathBuf,
    ino: u64,
    dev: u64,
}

impl IpcServerGuard {
    pub fn for_path(path: PathBuf) -> std::io::Result<Self> {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::metadata(&path)?;
        Ok(Self {
            path,
            ino: meta.ino(),
            dev: meta.dev(),
        })
    }
}

impl Drop for IpcServerGuard {
    fn drop(&mut self) {
        use std::os::unix::fs::MetadataExt;
        if let Ok(meta) = std::fs::metadata(&self.path)
            && meta.ino() == self.ino
            && meta.dev() == self.dev
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Converts a millisecond timeout into the [`Timespec`] expected by `poll(2)`.
fn ipc_timeout_spec(timeout_ms: i32) -> Timespec {
    let ms = timeout_ms.max(1);
    Timespec {
        tv_sec: i64::from(ms / 1000),
        tv_nsec: (i64::from(ms % 1000) * 1_000_000) as _,
    }
}

pub fn read_ipc_request(
    stream: &mut UnixStream,
    deadline: std::time::Instant,
    max_bytes: usize,
) -> std::io::Result<String> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];

    loop {
        let now = std::time::Instant::now();
        if now >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "IPC request timed out",
            ));
        }

        let remaining = deadline.saturating_duration_since(now);
        let timeout_ms = remaining.as_millis().min(i32::MAX as u128) as i32;

        let mut poll_fd = PollFd::from_borrowed_fd(stream.as_fd(), PollFlags::IN);
        let timeout_spec = ipc_timeout_spec(timeout_ms);

        let ready =
            match rustix::event::poll(std::slice::from_mut(&mut poll_fd), Some(&timeout_spec)) {
                Ok(ready) => ready,
                Err(rustix::io::Errno::INTR) => continue,
                Err(err) => return Err(std::io::Error::from(err)),
            };
        if ready == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "IPC request timed out",
            ));
        }

        match stream.read(&mut chunk) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "Connection closed before newline",
                ));
            }
            Ok(n) => {
                if buffer.len() + n > max_bytes {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "IPC request exceeded maximum allowed size",
                    ));
                }
                buffer.extend_from_slice(&chunk[..n]);
                if let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                    let line = std::str::from_utf8(&buffer[..pos])
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
                        .to_string();
                    return Ok(line);
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
}

pub fn write_ipc_response(
    stream: &mut UnixStream,
    data: &[u8],
    deadline: std::time::Instant,
) -> std::io::Result<()> {
    let mut written = 0;
    while written < data.len() {
        let now = std::time::Instant::now();
        if now >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "IPC response write timed out",
            ));
        }

        let remaining = deadline.saturating_duration_since(now);
        let timeout_ms = remaining.as_millis().min(i32::MAX as u128) as i32;

        let mut poll_fd = PollFd::from_borrowed_fd(stream.as_fd(), PollFlags::OUT);
        let timeout_spec = ipc_timeout_spec(timeout_ms);

        let ready =
            match rustix::event::poll(std::slice::from_mut(&mut poll_fd), Some(&timeout_spec)) {
                Ok(ready) => ready,
                Err(rustix::io::Errno::INTR) => continue,
                Err(err) => return Err(std::io::Error::from(err)),
            };
        if ready == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "IPC response write timed out",
            ));
        }

        match stream.write(&data[written..]) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "Failed to write to IPC socket",
                ));
            }
            Ok(n) => {
                written += n;
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    let _ = stream.flush();
    Ok(())
}
