use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

static LOG_FILE: OnceLock<Mutex<File>> = OnceLock::new();
static START: OnceLock<Instant> = OnceLock::new();

pub fn data_dir() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        let dir = PathBuf::from(appdata).join("Singularity");
        if fs::create_dir_all(&dir).is_ok() {
            return dir;
        }
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("data")
}

pub fn init() {
    START.get_or_init(Instant::now);
    let dir = data_dir();
    let _ = fs::create_dir_all(&dir);
    if let Ok(file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("singularity.log"))
    {
        let _ = LOG_FILE.set(Mutex::new(file));
    }
    info(&format!(
        "--- singularity start pid={} ---",
        std::process::id()
    ));
}

fn stamp() -> String {
    let elapsed = START
        .get()
        .map(|s| s.elapsed().as_secs_f64())
        .unwrap_or(0.0);
    let wall = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() % 86400)
        .unwrap_or(0);
    format!(
        "{:02}:{:02}:{:02}.{:03} (+{:.3}s)",
        wall / 3600,
        (wall / 60) % 60,
        wall % 60,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_millis())
            .unwrap_or(0),
        elapsed
    )
}

pub fn write(level: &str, message: &str) {
    let line = format!("[{}] {:<5} {}\n", stamp(), level, message);
    if let Some(file) = LOG_FILE.get() {
        if let Ok(mut file) = file.lock() {
            let _ = file.write_all(line.as_bytes());
            let _ = file.flush();
        }
    }
    #[cfg(debug_assertions)]
    eprint!("{}", line);
}

pub fn info(message: &str) {
    write("INFO", message);
}

pub fn warn(message: &str) {
    write("WARN", message);
}

pub fn error(message: &str) {
    write("ERROR", message);
}
