use std::{
    io::{self, Write as _},
    time::Duration,
};

fn main() -> io::Result<()> {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--panic")) {
        panic!("synthetic package runtime panic");
    }
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--exit-failure")) {
        std::process::exit(23);
    }
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--wait-for-cancel")) {
        std::thread::sleep(Duration::from_secs(300));
        return Ok(());
    }

    let mut stdout = io::stdout().lock();
    stdout.write_all(b"authorized command fixture\n")?;
    stdout.flush()
}
