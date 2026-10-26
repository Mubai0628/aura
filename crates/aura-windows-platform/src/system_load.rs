//! How busy the machine is: physical memory and the processors' busy share since the previous
//! sample. Background work such as building a workspace index paces itself by these.
use std::{io, mem::size_of};

use windows_sys::Win32::{
    Foundation::FILETIME,
    System::{
        SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX},
        Threading::GetSystemTimes,
    },
};

/// Physical memory, in bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalMemory {
    pub total: u64,
    pub available: u64,
}

pub fn physical_memory() -> io::Result<PhysicalMemory> {
    let mut status = MEMORYSTATUSEX {
        dwLength: u32::try_from(size_of::<MEMORYSTATUSEX>())
            .map_err(|_| io::Error::other("MEMORYSTATUSEX size overflow"))?,
        ..Default::default()
    };
    // SAFETY: `status` is a live MEMORYSTATUSEX with its length set, as the call requires.
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(PhysicalMemory { total: status.ullTotalPhys, available: status.ullAvailPhys })
}

/// Samples the processors' idle and busy time; each [`CpuLoad::busy_percent`] reports the busy
/// share since the sample before it.
#[derive(Debug)]
pub struct CpuLoad {
    idle: u64,
    total: u64,
}

impl CpuLoad {
    pub fn start() -> io::Result<Self> {
        let (idle, total) = system_times()?;
        Ok(Self { idle, total })
    }

    /// The busy share of all processors since the previous sample, 0–100.
    pub fn busy_percent(&mut self) -> io::Result<u8> {
        let (idle, total) = system_times()?;
        let (idle_delta, total_delta) =
            (idle.saturating_sub(self.idle), total.saturating_sub(self.total));
        (self.idle, self.total) = (idle, total);
        if total_delta == 0 {
            return Ok(0);
        }
        let busy = total_delta.saturating_sub(idle_delta).saturating_mul(100) / total_delta;
        Ok(u8::try_from(busy.min(100)).unwrap_or(100))
    }
}

/// Idle time and all time (kernel time includes idle), in 100 ns units.
fn system_times() -> io::Result<(u64, u64)> {
    let (mut idle, mut kernel, mut user) =
        (FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: the three out pointers refer to live FILETIME values for the call's duration.
    if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let value =
        |time: FILETIME| u64::from(time.dwHighDateTime) << 32 | u64::from(time.dwLowDateTime);
    Ok((value(idle), value(kernel).saturating_add(value(user))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_and_processor_load_are_read() {
        let memory = physical_memory().expect("memory status");
        assert!(memory.total > 0 && memory.available <= memory.total);
        let mut load = CpuLoad::start().expect("processor times");
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(load.busy_percent().expect("busy share") <= 100);
    }
}
