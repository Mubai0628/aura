//! The setup and uninstall windows worked with the keyboard alone, against a host inside a
//! temporary folder; the windows stay hidden and the keys go to them as a keyboard's would.
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    thread,
    time::{Duration, Instant},
};

use aura_pixel_window::{
    PixelWindow,
    testing::{Key, Part, hide_windows, parts, press, wait_for_heading, wait_for_window},
};
use aura_runtime_contract::BundleNamespace;

use super::{Running, Work, default_locations};
use crate::{
    host::Host,
    wizard::{self, Outside},
};

const WAIT: Duration = Duration::from_secs(30);

static WINDOWS: Mutex<()> = Mutex::new(());

/// The tests find the window by enumerating this process's, so one runs at a time.
fn one_at_a_time() -> MutexGuard<'static, ()> {
    hide_windows();
    WINDOWS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Stands in for Notepad, the installed launcher and the folder picker.
#[derive(Default)]
struct Recorder {
    started: Mutex<Vec<PathBuf>>,
    licenses: Mutex<usize>,
    /// What the folder picker returns, in turn; `None` once they run out.
    folders: Mutex<VecDeque<PathBuf>>,
}

impl Outside for Recorder {
    fn show_licenses(&self, _program: &Path) {
        *self.licenses.lock().unwrap() += 1;
    }

    fn start(&self, root: &Path) {
        self.started.lock().unwrap().push(root.to_path_buf());
    }

    fn choose_folder(&self, _window: &PixelWindow, _title: &str) -> Option<PathBuf> {
        self.folders.lock().unwrap().pop_front()
    }
}

fn keys(window: isize, keys: &[Key]) {
    for key in keys {
        press(window, *key);
    }
}

fn heading(window: isize, heading: &str) {
    assert!(
        wait_for_heading(window, heading, WAIT),
        "the window never showed {heading:?}: {:#?}",
        parts(window)
    );
}

/// Waits until a part of the window matches.
fn until(window: isize, what: &str, matches: impl Fn(&Part) -> bool) -> Part {
    let start = Instant::now();
    while start.elapsed() < WAIT {
        if let Some(part) = parts(window).ok().and_then(|parts| parts.into_iter().find(&matches)) {
            return part;
        }
        thread::sleep(Duration::from_millis(30));
    }
    panic!("the window never showed {what}: {:#?}", parts(window));
}

fn installing(
    work: &Work,
    setup: PathBuf,
    outside: &Arc<Recorder>,
) -> thread::JoinHandle<anyhow::Result<()>> {
    let (host, outside) = (work.host(), Arc::clone(outside));
    thread::spawn(move || wizard::install(&host, &setup, &*outside))
}

fn uninstalling(work: &Work) -> thread::JoinHandle<anyhow::Result<()>> {
    let (host, root) = (work.host(), work.root());
    thread::spawn(move || wizard::uninstall(&host, &root))
}

#[test]
fn the_keyboard_alone_installs_with_a_desktop_shortcut_and_starts_aura() {
    let _one = one_at_a_time();
    let work = Work::new();
    let outside = Arc::new(Recorder::default());
    let running = installing(&work, work.release("0.3.0"), &outside);
    let window = wait_for_window(WAIT).expect("the setup window appears");
    heading(window, "安装 Aura 0.3.0");
    let names: Vec<(&str, String)> =
        parts(window).unwrap().into_iter().map(|part| (part.role, part.name)).collect();
    for wanted in [
        ("button", "更改安装位置".to_owned()),
        ("button", "更改数据位置".to_owned()),
        ("check box", "在桌面创建快捷方式".to_owned()),
        ("button", "取消".to_owned()),
        ("button", "安装".to_owned()),
    ] {
        assert!(names.contains(&wanted), "{wanted:?} is not among {names:#?}");
    }
    // The focus starts on 安装; Tab goes round to the two 更改, then the box; Enter on the box
    // installs.
    keys(window, &[Key::Tab, Key::Tab, Key::Tab, Key::Space, Key::Enter]);
    heading(window, "Aura 0.3.0 已安装");
    keys(window, &[Key::Enter]);
    running.join().unwrap().unwrap();

    let host = work.host();
    assert_eq!(work.state().active, "0.3.0");
    assert!(host.desktop().unwrap().join("Aura Test setup.lnk").is_file());
    assert!(host.start_menu().unwrap().join("Aura Test setup.lnk").is_file());
    assert_eq!(*outside.started.lock().unwrap(), [work.root()]);
}

#[test]
fn a_chosen_folder_that_cannot_take_aura_is_named_on_the_page_and_another_can_be_chosen() {
    let _one = one_at_a_time();
    let work = Work::new();
    let taken = work.folder.path().join("taken").join("Aura");
    fs::create_dir_all(&taken).unwrap();
    fs::write(taken.join("someone else's.txt"), b"keep").unwrap();
    let free = work.folder.path().join("free");
    let outside = Arc::new(Recorder::default());
    outside.folders.lock().unwrap().extend([taken.clone(), free.clone()]);
    let running = installing(&work, work.release("0.3.0"), &outside);
    let window = wait_for_window(WAIT).expect("the setup window appears");
    heading(window, "安装 Aura 0.3.0");

    // From 安装, Tab goes round to 更改; Space presses it.
    keys(window, &[Key::Tab, Key::Space]);
    until(window, "the refusal", |part| part.role == "text" && part.name.contains("不是空文件夹"));
    keys(window, &[Key::Space]);
    let chosen = free.join("Aura");
    until(window, "the new location", |part| {
        part.role == "edit" && part.value.as_deref() == Some(&*chosen.display().to_string())
    });
    let shown = parts(window).unwrap();
    assert!(
        !shown.iter().any(|part| part.name.contains("不是空文件夹")),
        "the refusal stayed: {shown:#?}"
    );
    // Down moves past the data's 更改 to the box; Enter there installs without ticking it.
    keys(window, &[Key::Down, Key::Down, Key::Enter]);
    heading(window, "Aura 0.3.0 已安装");
    keys(window, &[Key::Escape]);
    running.join().unwrap().unwrap();

    assert!(chosen.join("active.json").is_file());
    assert!(!work.host().desktop().unwrap().join("Aura Test setup.lnk").exists());
    assert_eq!(fs::read(taken.join("someone else's.txt")).unwrap(), b"keep");
    assert!(outside.started.lock().unwrap().is_empty(), "Esc on the last page started Aura");
}

#[test]
fn the_last_page_removes_the_ticked_old_data_and_keeps_what_was_unticked() {
    let _one = one_at_a_time();
    let work = Work::new();
    let roots = work.host().roots().unwrap();
    // The layout before namespaces, and a namespace no installation uses.
    let early = roots.local_aura().join("data");
    fs::create_dir_all(&early).unwrap();
    fs::write(early.join("aura.db"), b"early").unwrap();
    let orphan: BundleNamespace = "aura.desktop.test.orphan".parse().unwrap();
    let [namespaces, _] = roots.namespace_folders();
    let orphan_data = namespaces.join(orphan.object_name_hash()).join("data");
    fs::create_dir_all(&orphan_data).unwrap();
    fs::write(orphan_data.join("aura-v2.db"), b"orphan").unwrap();
    let outside = Arc::new(Recorder::default());
    let running = installing(&work, work.release("0.3.0"), &outside);
    let window = wait_for_window(WAIT).expect("the setup window appears");
    heading(window, "安装 Aura 0.3.0");
    keys(window, &[Key::Enter]);
    heading(window, "Aura 0.3.0 已安装");
    let first = until(window, "the first box", |part| part.automation_id == "check-old-0");
    assert!(first.name.starts_with("早期版本的数据") && first.checked == Some(true), "{first:?}");
    let second = until(window, "the second box", |part| part.automation_id == "check-old-1");
    assert!(second.name.starts_with("旧安装的数据") && second.checked == Some(true), "{second:?}");

    // 完成 has the focus; Tab goes round to the first box, Down to the second, Space unticks it.
    keys(window, &[Key::Tab, Key::Down, Key::Space]);
    until(window, "the unticked box", |part| {
        part.automation_id == "check-old-1" && part.checked == Some(false)
    });
    // Down past 现在启动 Aura to 删除所选; Enter presses it.
    keys(window, &[Key::Down, Key::Down, Key::Enter]);
    until(window, "what was removed", |part| part.name == "已删除：早期版本的数据");
    let left = until(window, "what is left", |part| part.automation_id == "check-old-0");
    assert!(left.name.starts_with("旧安装的数据") && left.checked == Some(false), "{left:?}");
    let remove = until(window, "删除所选", |part| part.automation_id == "button-remove-old");
    assert!(!remove.enabled, "nothing is ticked: {remove:?}");
    // The page changed, so the focus is back on 完成.
    keys(window, &[Key::Enter]);
    running.join().unwrap().unwrap();

    assert!(!early.exists());
    assert_eq!(fs::read(orphan_data.join("aura-v2.db")).unwrap(), b"orphan");
    assert_eq!(*outside.started.lock().unwrap(), [work.root()]);
}

#[test]
fn the_keyboard_alone_uninstalls_with_the_data_after_one_confirmation() {
    let _one = one_at_a_time();
    let work = Work::new();
    work.install(&work.release("0.3.0"), None, true).unwrap();
    let host = work.host();
    let data = default_locations(&host).data_root;
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("aura-v2.db"), b"conversations").unwrap();
    let running = uninstalling(&work);
    let window = wait_for_window(WAIT).expect("the uninstall window appears");
    heading(window, "卸载 Aura 0.3.0");
    let remove =
        until(window, "the uninstall button", |part| part.automation_id == "button-remove");
    assert_eq!(remove.name, "卸载");

    // From 卸载, Tab goes round to the box; ticking it turns the button into the danger one.
    keys(window, &[Key::Tab, Key::Space]);
    until(window, "the danger button", |part| {
        part.automation_id == "button-remove" && part.name == "删除并卸载"
    });
    let check = until(window, "the box", |part| part.automation_id == "check-delete-data");
    assert_eq!(check.checked, Some(true));
    // The focus stayed on the box: Tab to 取消, Tab to 删除并卸载, Enter presses it.
    keys(window, &[Key::Tab, Key::Tab, Key::Enter]);
    heading(window, "Aura 已卸载");
    keys(window, &[Key::Enter]);
    running.join().unwrap().unwrap();

    assert!(!work.root().exists());
    assert!(!data.exists());
    assert!(!host.desktop().unwrap().join("Aura Test setup.lnk").exists());
}

#[test]
fn enter_alone_uninstalls_and_keeps_the_data_once_aura_is_closed() {
    let _one = one_at_a_time();
    let work = Work::new();
    work.install(&work.release("0.3.0"), None, false).unwrap();
    let data = default_locations(&work.host()).data_root;
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("aura-v2.db"), b"conversations").unwrap();
    // Aura's programs run from its version folders; a file of another name directly in the
    // installation's folder is not Aura's, and the folder would stay for it.
    let aura = Running::start(&work.root().join(r"versions\0.3.0"));
    let running = uninstalling(&work);
    let window = wait_for_window(WAIT).expect("the uninstall window appears");
    heading(window, "卸载 Aura 0.3.0");
    keys(window, &[Key::Enter]);
    heading(window, "Aura 正在运行");
    assert!(work.root().exists());
    drop(aura);
    let (host, start) = (work.host(), Instant::now());
    while !host.running_in(&work.root()).unwrap().is_empty() {
        assert!(start.elapsed() < WAIT, "the stand-in for Aura kept running");
        thread::sleep(Duration::from_millis(50));
    }
    // 重试 has the focus.
    keys(window, &[Key::Enter]);
    heading(window, "Aura 已卸载");
    keys(window, &[Key::Enter]);
    running.join().unwrap().unwrap();

    assert!(!work.root().exists());
    assert_eq!(fs::read(data.join("aura-v2.db")).unwrap(), b"conversations");
}
