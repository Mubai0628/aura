//! The release a setup program carries: a zip of the install payload (`Aura.ico` and
//! `versions/<version>/...`, which holds the launcher and the setup program) appended to the program, then a 32-byte trailer — the
//! zip's offset and length, then `MAGIC` — that says where it is. The same zip, alone, is the
//! update package.
use std::{
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use zip::ZipArchive;

pub const MAGIC: &[u8; 16] = b"AuraSetupPayload";
const TRAILER: u64 = 32;
pub const LAUNCHER: &str = "AuraLauncher.exe";
pub const ICON: &str = "Aura.ico";

/// Where the zip sits inside a setup program.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Span {
    pub offset: u64,
    pub length: u64,
}

pub fn locate(program: &Path) -> Result<Span> {
    let mut file =
        File::open(program).with_context(|| format!("cannot open {}", program.display()))?;
    let size = file.metadata()?.len();
    ensure!(size > TRAILER, "this program carries no Aura release");
    file.seek(SeekFrom::Start(size - TRAILER))?;
    let mut trailer = [0_u8; TRAILER as usize];
    file.read_exact(&mut trailer)?;
    ensure!(&trailer[16..] == MAGIC, "this program carries no Aura release");
    let number = |bytes: &[u8]| u64::from_le_bytes(bytes.try_into().unwrap_or_default());
    let span = Span { offset: number(&trailer[..8]), length: number(&trailer[8..16]) };
    ensure!(
        span.offset.checked_add(span.length).and_then(|end| end.checked_add(TRAILER)) == Some(size),
        "the release inside this program is damaged"
    );
    Ok(span)
}

/// The bytes of a file between two offsets, read and sought like a file of their own.
pub struct Region {
    file: File,
    span: Span,
    position: u64,
}

impl Region {
    pub fn open(path: &Path, span: Span) -> Result<Self> {
        Ok(Self { file: File::open(path)?, span, position: 0 })
    }
}

impl Read for Region {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let left = self.span.length.saturating_sub(self.position);
        let wanted = buffer.len().min(usize::try_from(left).unwrap_or(usize::MAX));
        if wanted == 0 {
            return Ok(0);
        }
        self.file.seek(SeekFrom::Start(self.span.offset + self.position))?;
        let read = self.file.read(&mut buffer[..wanted])?;
        self.position += read as u64;
        Ok(read)
    }
}

impl Seek for Region {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let target = match to {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::End(delta) => self.span.length.checked_add_signed(delta),
            SeekFrom::Current(delta) => self.position.checked_add_signed(delta),
        };
        self.position = target.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "seek outside the release")
        })?;
        Ok(self.position)
    }
}

/// A release archive: inside a setup program, or an update package on its own.
pub type Archive = ZipArchive<Region>;

pub fn open_in_program(program: &Path) -> Result<Archive> {
    let span = locate(program)?;
    Ok(ZipArchive::new(Region::open(program, span)?)?)
}

pub fn open_package(package: &Path) -> Result<Archive> {
    let length = fs::metadata(package)?.len();
    Ok(ZipArchive::new(Region::open(package, Span { offset: 0, length })?)?)
}

/// Writes `stub` followed by `zip` and the trailer to `out`.
pub fn append(stub: &[u8], zip: &Path, out: &Path) -> Result<()> {
    let mut file = File::create(out)?;
    file.write_all(stub)?;
    let length = io::copy(&mut File::open(zip)?, &mut file)?;
    let offset = stub.len() as u64;
    file.write_all(&offset.to_le_bytes())?;
    file.write_all(&length.to_le_bytes())?;
    file.write_all(MAGIC)?;
    file.sync_all()?;
    Ok(())
}

/// The setup program without the release it carries, used as the installed uninstaller.
pub fn program_without_release(program: &Path) -> Result<Vec<u8>> {
    let span = locate(program).ok();
    let mut bytes = fs::read(program)?;
    if let Some(span) = span {
        bytes.truncate(usize::try_from(span.offset)?);
    }
    Ok(bytes)
}

/// The one version an archive carries.
pub fn version_of(archive: &mut Archive) -> Result<String> {
    let mut versions = std::collections::BTreeSet::new();
    for name in archive.file_names() {
        if let Some(rest) = name.strip_prefix("versions/")
            && let Some((version, _)) = rest.split_once('/')
        {
            versions.insert(version.to_owned());
        }
    }
    ensure!(
        versions.len() == 1,
        "the release must carry exactly one version, found {}",
        versions.len()
    );
    versions.pop_first().context("the release carries no version")
}

pub fn read_entry(archive: &mut Archive, name: &str) -> Result<Option<Vec<u8>>> {
    let mut entry = match archive.by_name(name) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or(0));
    entry.read_to_end(&mut bytes)?;
    Ok(Some(bytes))
}

/// The bytes the archive's files take once extracted.
pub fn extracted_size(archive: &mut Archive) -> Result<u64> {
    let mut total = 0_u64;
    for index in 0..archive.len() {
        total = total.saturating_add(archive.by_index_raw(index)?.size());
    }
    Ok(total)
}

/// Extracts every file into `destination`, keeping the archive's folders. Names that would leave
/// the destination, links and duplicate names are refused. `progress` gets bytes done and total.
pub fn extract(
    archive: &mut Archive,
    destination: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Vec<PathBuf>> {
    let total = extracted_size(archive)?;
    let mut done = 0_u64;
    let mut written = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let Some(relative) = entry.enclosed_name() else {
            bail!("the release holds an unsafe path: {}", entry.name());
        };
        if entry.is_symlink() {
            bail!("the release holds a link: {}", entry.name());
        }
        if !seen.insert(relative.to_string_lossy().to_lowercase()) {
            bail!("the release holds a name twice: {}", entry.name());
        }
        let path = destination.join(&relative);
        if entry.is_dir() {
            fs::create_dir_all(&path)?;
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file =
            File::create(&path).with_context(|| format!("cannot write {}", path.display()))?;
        let mut buffer = vec![0_u8; 1 << 20];
        loop {
            let read = entry.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read])?;
            done += read as u64;
            progress(done, total);
        }
        written.push(relative);
    }
    Ok(written)
}
