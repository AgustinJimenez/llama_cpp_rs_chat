use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;

lazy_static::lazy_static! {
    static ref LOG_FILE: Mutex<File> = Mutex::new(
        OpenOptions::new()
            .create(true)
            .append(true)
            .open("debug.log")
            .unwrap()
    );
}

pub fn debug_log(message: &str) {
    let mut file = LOG_FILE.lock().unwrap();
    writeln!(file, "{}", message).unwrap();
}
