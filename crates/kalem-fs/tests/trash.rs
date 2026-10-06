//! Trashing and restoring, against a trash in a temporary home: the test
//! runs itself again with the environment pointing there.

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn trash_and_restore() {
    let Some(home) = std::env::var_os("KALEM_TEST_TRASH_HOME") else {
        let dir = std::env::temp_dir().join(format!("kalem-trash-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("data")).unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "trash_and_restore", "--nocapture"])
            .env("KALEM_TEST_TRASH_HOME", &dir)
            .env("HOME", &dir)
            .env("XDG_DATA_HOME", dir.join("data"))
            .status()
            .unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert!(status.success());
        return;
    };
    let dir = std::path::PathBuf::from(home).join("work");
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("a.txt");
    std::fs::write(&a, "first").unwrap();
    kalem_fs::trash_paths(std::slice::from_ref(&a)).unwrap();
    // Deletion times are in seconds.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(&a, "second").unwrap();
    kalem_fs::trash_paths(std::slice::from_ref(&a)).unwrap();
    assert!(!a.exists());
    // The latest one comes back.
    kalem_fs::restore(std::slice::from_ref(&a)).unwrap();
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "second");
    // Not over a file that exists.
    assert!(kalem_fs::restore(std::slice::from_ref(&a)).is_err());
    std::fs::remove_file(&a).unwrap();
    kalem_fs::restore(std::slice::from_ref(&a)).unwrap();
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "first");
    assert!(kalem_fs::restore(&[dir.join("never.txt")]).is_err());
}

/// A trash folder of the test's own, as the editors' tests use: trashed
/// files go there and come back from there, the latest first.
#[test]
fn a_trash_folder_of_its_own() {
    let dir = std::env::temp_dir().join(format!("kalem-trash-folder-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    kalem_fs::set_trash_dir(Some(dir.join("trash")));
    let a = work.join("a.txt");
    std::fs::write(&a, "first").unwrap();
    kalem_fs::trash_paths(std::slice::from_ref(&a)).unwrap();
    std::fs::write(&a, "second").unwrap();
    kalem_fs::trash_paths(std::slice::from_ref(&a)).unwrap();
    assert!(!a.exists());
    assert_eq!(std::fs::read_dir(dir.join("trash")).unwrap().count(), 2);
    kalem_fs::restore(std::slice::from_ref(&a)).unwrap();
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "second");
    assert!(kalem_fs::restore(std::slice::from_ref(&a)).is_err());
    std::fs::remove_file(&a).unwrap();
    kalem_fs::restore(std::slice::from_ref(&a)).unwrap();
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "first");
    // Trashed again after a restore: found again as the latest.
    kalem_fs::trash_paths(std::slice::from_ref(&a)).unwrap();
    kalem_fs::restore(std::slice::from_ref(&a)).unwrap();
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "first");
    assert!(kalem_fs::restore(&[work.join("never.txt")]).is_err());
    kalem_fs::set_trash_dir(None);
    std::fs::remove_dir_all(&dir).unwrap();
}
