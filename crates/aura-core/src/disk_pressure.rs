//! How many reads of the store run at once, by how busy the disk under it is.
//!
//! Every read has a read-only connection of its own (`agent_store::readers`), so a slow read holds
//! up no other. Reading more at once while the disk is already slow only adds to what it queues,
//! the store's own writes included, so the number follows the system's average time per transfer
//! on the store's volume: at most [`MOST_READS`], never fewer than [`FEWEST_READS`] - fewer would
//! let one slow read hold up the rest exactly when reads are slowest. It drops after two busy
//! samples in a row and rises one at a time after ten calm seconds, so a burst does not swing it.
//! It starts, and stays without a reading, at [`FEWEST_READS`].
use std::{
    path::{Component, Path, Prefix},
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

/// Reads that may run at once while the disk is calm; also the size of the read lane.
pub(crate) const MOST_READS: usize = 4;
/// Reads that may run at once however busy the disk is.
pub(crate) const FEWEST_READS: usize = 2;

const SAMPLE_EVERY: Duration = Duration::from_secs(2);
/// An average transfer this slow means the disk is busy.
const BUSY_MS: f64 = 25.0;
/// And this slow, very busy.
const VERY_BUSY_MS: f64 = 100.0;
/// Busy samples in a row before fewer reads run at once.
const LOWER_AFTER: u32 = 2;
/// Calm samples in a row before one more read runs at once.
const RAISE_AFTER: u32 = 5;

static LIMIT: AtomicUsize = AtomicUsize::new(FEWEST_READS);
static WATCHING: OnceLock<()> = OnceLock::new();

/// How many reads may run at once now. Core's own tests do not depend on how busy the machine's
/// disk is: they read at the most.
pub(crate) fn read_limit() -> usize {
    if cfg!(test) { MOST_READS } else { LIMIT.load(Ordering::Relaxed) }
}

/// Starts following the disk `database` is on, once per process.
pub(crate) fn watch(database: &Path) {
    if cfg!(test) {
        return;
    }
    WATCHING.get_or_init(|| {
        let Some(volume) = volume_of(database) else {
            note(FEWEST_READS, None, Some("the store is not on a drive letter"));
            return;
        };
        let started = std::thread::Builder::new()
            .name("aura-disk-pressure".to_owned())
            .spawn(move || follow(&volume));
        if let Err(error) = started {
            note(FEWEST_READS, None, Some(&error.to_string()));
        }
    });
}

fn follow(volume: &str) {
    let mut load = match aura_windows_platform::DiskLoad::start(volume) {
        Ok(load) => load,
        Err(error) => {
            note(FEWEST_READS, None, Some(&error.to_string()));
            return;
        }
    };
    let mut steady = Steady::new();
    loop {
        std::thread::sleep(SAMPLE_EVERY);
        let transfer_ms = load.transfer_ms().ok().flatten();
        let before = steady.limit;
        let limit = steady.next(wanted(transfer_ms));
        if limit != before {
            LIMIT.store(limit, Ordering::Relaxed);
            note(limit, transfer_ms, None);
        }
    }
}

fn note(limit: usize, transfer_ms: Option<f64>, unavailable: Option<&str>) {
    crate::append_core_log(
        "store_read_limit",
        serde_json::json!({
            "limit": limit,
            "transfer_ms": transfer_ms.map(|ms| (ms * 10.0).round() / 10.0),
            "unavailable": unavailable,
        }),
    );
}

/// What one sample asks for.
fn wanted(transfer_ms: Option<f64>) -> usize {
    match transfer_ms {
        Some(ms) if ms < BUSY_MS => MOST_READS,
        Some(ms) if ms < VERY_BUSY_MS => MOST_READS - 1,
        _ => FEWEST_READS,
    }
}

/// The limit, moved only by samples that agree for a while.
struct Steady {
    limit: usize,
    lower: u32,
    raise: u32,
}

impl Steady {
    const fn new() -> Self {
        Self { limit: FEWEST_READS, lower: 0, raise: 0 }
    }

    fn next(&mut self, wanted: usize) -> usize {
        if wanted < self.limit {
            self.raise = 0;
            self.lower += 1;
            if self.lower >= LOWER_AFTER {
                self.limit = wanted;
                self.lower = 0;
            }
        } else if wanted > self.limit {
            self.lower = 0;
            self.raise += 1;
            if self.raise >= RAISE_AFTER {
                self.limit += 1;
                self.raise = 0;
            }
        } else {
            self.lower = 0;
            self.raise = 0;
        }
        self.limit
    }
}

/// The drive the store is on, as the disk counters name it (`C:`).
fn volume_of(path: &Path) -> Option<String> {
    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return None;
    };
    match prefix.kind() {
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
            Some(format!("{}:", char::from(letter.to_ascii_uppercase())))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(steady: &mut Steady, samples: &[Option<f64>]) -> Vec<usize> {
        samples.iter().map(|sample| steady.next(wanted(*sample))).collect()
    }

    #[test]
    fn a_calm_disk_reads_at_the_most_one_step_at_a_time() {
        let mut steady = Steady::new();
        let calm = [Some(1.0); 10];
        assert_eq!(run(&mut steady, &calm), [2, 2, 2, 2, 3, 3, 3, 3, 3, 4]);
        assert_eq!(steady.next(wanted(Some(0.0))), MOST_READS, "and stays there");
    }

    #[test]
    fn a_busy_disk_lowers_it_after_two_samples_and_one_spike_does_not() {
        let mut steady = Steady { limit: MOST_READS, lower: 0, raise: 0 };
        // One slow sample between calm ones moves nothing.
        assert_eq!(run(&mut steady, &[Some(150.0), Some(2.0), Some(150.0), Some(2.0)]), [4; 4]);
        // Two busy ones in a row: one fewer.
        assert_eq!(run(&mut steady, &[Some(40.0), Some(40.0)]), [4, 3]);
        // Very busy: the fewest, never below.
        assert_eq!(run(&mut steady, &[Some(400.0), Some(900.0), Some(900.0)]), [3, 2, 2]);
    }

    #[test]
    fn without_a_reading_the_fewest_run_at_once() {
        let mut steady = Steady { limit: MOST_READS, lower: 0, raise: 0 };
        assert_eq!(run(&mut steady, &[None, None]), [4, FEWEST_READS]);
        assert_eq!(Steady::new().limit, FEWEST_READS, "and it starts there");
    }

    #[test]
    fn the_volume_is_the_drive_the_store_is_on() {
        assert_eq!(
            volume_of(Path::new(r"C:\Users\a\AppData\Local\Aura\data\aura-v2.db")).as_deref(),
            Some("C:")
        );
        assert_eq!(volume_of(Path::new(r"\\?\d:\Aura\aura-v2.db")).as_deref(), Some("D:"));
        assert_eq!(volume_of(Path::new(r"\\server\share\aura-v2.db")), None);
        assert_eq!(volume_of(Path::new("aura-v2.db")), None);
    }
}
