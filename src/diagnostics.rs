use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::{Mutex, OnceLock},
};

static LOG_FILE: OnceLock<Mutex<Option<File>>> = OnceLock::new();

pub fn init(path: &Path) -> io::Result<()> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let _ = LOG_FILE.set(Mutex::new(Some(file)));
    Ok(())
}

pub fn record(level: &str, message: impl std::fmt::Display) {
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let line = format!("{timestamp} [{level}] {message}");

    if let Some(log_file) = LOG_FILE.get() {
        let mut file = log_file
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let write_result = file
            .as_mut()
            .map(|output| writeln!(output, "{line}").and_then(|()| output.flush()));
        if let Some(Err(error)) = write_result {
            eprintln!("Impossible d'écrire dans plotter.log : {error}");
            *file = None;
        } else if file.is_none() {
            eprintln!("{line}");
        }
    } else {
        eprintln!("{line}");
    }
}
