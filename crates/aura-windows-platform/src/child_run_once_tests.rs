use super::*;
use windows_sys::Win32::System::Registry::RegDeleteTreeW;
struct Fixture(String);
impl Fixture {
    fn new() -> Self {
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).expect("random fixture key");
        Self(format!(
            r"Software\Aura\Tests\ChildRunOnce-{}",
            random.iter().map(|byte| format!("{byte:02x}")).collect::<String>()
        ))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // SAFETY: only this test's randomized subtree is removed; never the real RunOnce key.
        unsafe {
            RegDeleteTreeW(HKEY_CURRENT_USER, wide(&self.0).as_ptr());
        }
    }
}
#[test]
fn a_temporary_startup_value_cleans_up_without_touching_neighbours() -> io::Result<()> {
    let fixture = Fixture::new();
    let key = open(&fixture.0)?;
    write(&key, "unrelated", "keep")?;
    {
        let _entry = ChildRunOnce::install_at(&fixture.0, "AuraChild-fixture", "fixture.exe")?;
        assert_eq!(read(&key, "AuraChild-fixture")?.as_deref(), Some("fixture.exe"));
    }
    assert_eq!(read(&key, "AuraChild-fixture")?, None);
    assert_eq!(read(&key, "unrelated")?.as_deref(), Some("keep"));
    Ok(())
}
#[test]
fn startup_collision_and_replaced_value_are_never_overwritten_or_removed() -> io::Result<()> {
    let fixture = Fixture::new();
    let key = open(&fixture.0)?;
    let mut entry = ChildRunOnce::install_at(&fixture.0, "AuraChild-fixture", "fixture.exe")?;
    assert!(ChildRunOnce::install_at(&fixture.0, "AuraChild-fixture", "other.exe").is_err());
    write(&key, "AuraChild-fixture", "changed.exe")?;
    assert!(entry.remove().is_err());
    drop(entry);
    assert_eq!(read(&key, "AuraChild-fixture")?.as_deref(), Some("changed.exe"));
    Ok(())
}
