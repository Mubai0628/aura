//! How busy a volume's disk is: the system's own average time per transfer on it, from the
//! performance counters every Windows keeps (`\LogicalDisk(X:)\Avg. Disk sec/Transfer`).
use std::{io, ptr};

use windows_sys::Win32::System::Performance::{
    PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE,
    PDH_HCOUNTER, PDH_HQUERY, PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData,
    PdhGetFormattedCounterValue, PdhOpenQueryW,
};

/// Samples one volume's average time per transfer; each [`DiskLoad::transfer_ms`] reports it
/// since the sample before.
#[derive(Debug)]
pub struct DiskLoad {
    query: PDH_HQUERY,
    counter: PDH_HCOUNTER,
}

impl DiskLoad {
    /// `volume` is a drive such as `C:`. Nothing else is taken: the counter path would read `*`
    /// or `?` as a wildcard over other volumes.
    pub fn start(volume: &str) -> io::Result<Self> {
        let [letter, b':'] = volume.as_bytes() else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "a drive such as C:"));
        };
        if !letter.is_ascii_alphabetic() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "a drive such as C:"));
        }
        let mut query: PDH_HQUERY = ptr::null_mut();
        // SAFETY: a null data source asks for live data; `query` receives the new handle.
        status(unsafe { PdhOpenQueryW(ptr::null(), 0, &mut query) })?;
        // From here the query is closed on every path, by `Drop`.
        let mut load = Self { query, counter: ptr::null_mut() };
        let path: Vec<u16> = format!("\\LogicalDisk({volume})\\Avg. Disk sec/Transfer")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        // SAFETY: `path` is NUL-terminated and outlives the call; the query handle is open and
        // `load.counter` receives the counter's handle, which the query owns.
        status(unsafe { PdhAddEnglishCounterW(load.query, path.as_ptr(), 0, &mut load.counter) })?;
        // A rate needs two samples: this is the first.
        // SAFETY: the query handle is open.
        status(unsafe { PdhCollectQueryData(load.query) })?;
        Ok(load)
    }

    /// The average milliseconds one transfer took since the previous sample; `None` when the
    /// system has no value for the interval.
    pub fn transfer_ms(&mut self) -> io::Result<Option<f64>> {
        // SAFETY: the query handle is open.
        status(unsafe { PdhCollectQueryData(self.query) })?;
        let mut value = PDH_FMT_COUNTERVALUE::default();
        // SAFETY: the counter belongs to this open query and `value` is a live out value.
        let code = unsafe {
            PdhGetFormattedCounterValue(self.counter, PDH_FMT_DOUBLE, ptr::null_mut(), &mut value)
        };
        if code != 0
            || (value.CStatus != PDH_CSTATUS_VALID_DATA && value.CStatus != PDH_CSTATUS_NEW_DATA)
        {
            return Ok(None);
        }
        // SAFETY: a value formatted as PDH_FMT_DOUBLE is the union's double.
        let seconds = unsafe { value.Anonymous.doubleValue };
        Ok(seconds.is_finite().then(|| seconds.max(0.0) * 1_000.0))
    }
}

impl Drop for DiskLoad {
    fn drop(&mut self) {
        // SAFETY: the query handle is open; closing it also frees the counter it owns.
        unsafe { PdhCloseQuery(self.query) };
    }
}

fn status(code: u32) -> io::Result<()> {
    if code == 0 { Ok(()) } else { Err(io::Error::other(format!("pdh status 0x{code:08x}"))) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_volume_reports_its_time_per_transfer() {
        let mut load = DiskLoad::start("C:").expect("the system volume's disk counters");
        std::thread::sleep(std::time::Duration::from_millis(300));
        if let Some(ms) = load.transfer_ms().expect("a sample") {
            assert!(ms.is_finite() && ms >= 0.0, "{ms}");
        }
        // Only a drive is taken: `?` would match another volume's counters.
        for not_a_drive in ["?:", "*:", "C:\\", "C", "_Total"] {
            assert!(DiskLoad::start(not_a_drive).is_err(), "{not_a_drive}");
        }
    }
}
