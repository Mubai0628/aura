//! A test-feature-only native image; never installed or packaged with Aura.
use std::{
    io::{self, Write},
    time::Duration,
};

fn main() -> io::Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("fixture-write-file") => std::fs::write(&args[1], b"forbidden\n")?,
        Some("fixture-spawn-child") => {
            match std::process::Command::new(std::env::current_exe()?).arg("fixture-child").status()
            {
                Ok(_) => return Err(io::Error::other("child process was allowed")),
                Err(error) => println!("child-denied: {error}"),
            }
        }
        Some("fixture-child") => println!("child-started"),
        Some("fixture-wait") => {
            println!("ready");
            io::stdout().flush()?;
            std::thread::sleep(Duration::from_secs(30));
        }
        Some("fixture-flood") => loop {
            io::stdout().write_all(b"bounded-output\n")?;
        },
        Some("fixture-trace-env") => {
            for key in ["GIT_TRACE2", "GIT_TRACE2_EVENT", "GIT_TRACE2_PERF"] {
                println!("{key}={}", std::env::var(key).unwrap_or_default());
            }
        }
        _ => {
            let path = std::env::current_exe()?.with_file_name("version.txt");
            print!("{}", std::fs::read_to_string(path)?);
        }
    }
    Ok(())
}
