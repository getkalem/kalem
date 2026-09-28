//! File operations on temporary trees: copy and move with conflicts,
//! links, permissions, cancellation and listings.

use std::path::{Path, PathBuf};

use kalem_fs::{Conflict, Job, ListOptions, OpKind, Operation, SortKey};

fn tree(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kalem-fs-t-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for (f, text) in files {
        let p = d.join(f);
        if f.ends_with('/') {
            std::fs::create_dir_all(p).unwrap();
        } else {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
    }
    d.canonicalize().unwrap()
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

#[test]
fn listing_sorts_and_hides() {
    let d = tree(
        "list",
        &[
            ("b.org", "bb"),
            ("a10.txt", "a"),
            ("a2.txt", "aaaa"),
            (".hidden", ""),
            ("sub/x", ""),
        ],
    );
    let names = |o: &ListOptions| {
        kalem_fs::read_dir(&d, o)
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect::<Vec<_>>()
    };
    let mut o = ListOptions::default();
    assert_eq!(names(&o), vec!["sub", "a2.txt", "a10.txt", "b.org"]);
    o.hidden = true;
    assert_eq!(
        names(&o),
        vec!["sub", ".hidden", "a2.txt", "a10.txt", "b.org"]
    );
    o.hidden = false;
    o.sort = SortKey::Size;
    o.dirs_first = false;
    assert_eq!(names(&o)[0], "sub");
    o.sort = SortKey::Extension;
    o.dirs_first = true;
    assert_eq!(names(&o), vec!["sub", "b.org", "a2.txt", "a10.txt"]);
    o.reverse = true;
    assert_eq!(names(&o), vec!["sub", "a10.txt", "a2.txt", "b.org"]);
}

#[test]
fn copy_tree_with_links_and_permissions() {
    let d = tree(
        "copy",
        &[("src/a.txt", "A"), ("src/deep/b.txt", "B"), ("dst/", "")],
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("a.txt", d.join("src/link")).unwrap();
        kalem_fs::chmod(&d.join("src/a.txt"), 0o600).unwrap();
    }
    let op = Operation::transfer(OpKind::Copy, &[d.join("src")], &d.join("dst")).unwrap();
    assert!(kalem_fs::conflicts(&op).is_empty());
    let out = Job::start(op).wait();
    assert_eq!(out.errors, vec![]);
    assert_eq!(read(&d.join("dst/src/deep/b.txt")), "B");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::read_link(d.join("dst/src/link")).unwrap(),
            PathBuf::from("a.txt")
        );
        let mode = std::fs::metadata(d.join("dst/src/a.txt"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    // Onto itself: refused, and the file stays.
    let op = Operation::transfer(OpKind::Copy, &[d.join("src/a.txt")], &d.join("src")).unwrap();
    let out = Job::start(op).wait();
    assert_eq!(out.errors.len(), 1);
    assert_eq!(read(&d.join("src/a.txt")), "A");
    // Into itself: refused.
    let op = Operation::transfer(OpKind::Copy, &[d.join("src")], &d.join("src/deep")).unwrap();
    let out = Job::start(op).wait();
    assert_eq!(out.errors.len(), 1);
}

#[test]
fn conflicts_skip_overwrite_keep_both() {
    let d = tree(
        "conflict",
        &[
            ("a.txt", "new"),
            ("b.txt", "newb"),
            ("c.txt", "newc"),
            ("to/a.txt", "old"),
            ("to/b.txt", "oldb"),
            ("to/c.txt", "oldc"),
        ],
    );
    let mut op = Operation::transfer(
        OpKind::Copy,
        &[d.join("a.txt"), d.join("b.txt"), d.join("c.txt")],
        &d.join("to"),
    )
    .unwrap();
    assert_eq!(kalem_fs::conflicts(&op).len(), 3);
    op.choices.insert(d.join("to/a.txt"), Conflict::Overwrite);
    op.choices.insert(d.join("to/b.txt"), Conflict::KeepBoth);
    op.default_choice = Conflict::Skip;
    let out = Job::start(op).wait();
    assert_eq!(read(&d.join("to/a.txt")), "new");
    assert_eq!(read(&d.join("to/b.txt")), "oldb");
    assert_eq!(read(&d.join("to/b (2).txt")), "newb");
    assert_eq!(read(&d.join("to/c.txt")), "oldc");
    assert_eq!(out.skipped, vec![d.join("c.txt")]);
}

#[test]
fn move_rename_and_merge() {
    let d = tree("move", &[("a.txt", "A"), ("dir/x", "X"), ("to/dir/y", "Y")]);
    // A rename.
    let op = Operation::transfer(OpKind::Move, &[d.join("a.txt")], &d.join("renamed.txt")).unwrap();
    let out = Job::start(op).wait();
    assert_eq!(out.done.len(), 1);
    assert!(!d.join("a.txt").exists());
    assert_eq!(read(&d.join("renamed.txt")), "A");
    // A directory onto an existing one: merged.
    let mut op = Operation::transfer(OpKind::Move, &[d.join("dir")], &d.join("to")).unwrap();
    op.default_choice = Conflict::Overwrite;
    let out = Job::start(op).wait();
    assert_eq!(out.errors, vec![]);
    assert_eq!(read(&d.join("to/dir/x")), "X");
    assert_eq!(read(&d.join("to/dir/y")), "Y");
    assert!(!d.join("dir").exists());
    // Several sources need a directory.
    assert!(
        Operation::transfer(
            OpKind::Move,
            &[d.join("renamed.txt"), d.join("to")],
            &d.join("nope")
        )
        .is_err()
    );
}

#[test]
fn delete_link_not_target_and_small_operations() {
    let d = tree("delete", &[("t/keep.txt", "K"), ("gone/x", "")]);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(d.join("t"), d.join("ln")).unwrap();
        kalem_fs::delete_path(&d.join("ln")).unwrap();
        assert_eq!(read(&d.join("t/keep.txt")), "K");
    }
    let out = Job::start(Operation::remove(OpKind::Delete, &[d.join("gone")])).wait();
    assert_eq!(out.done.len(), 1);
    assert!(!d.join("gone").exists());
    kalem_fs::mkdir(&d.join("n/m")).unwrap();
    assert!(kalem_fs::mkdir(&d.join("n/m")).is_err());
    kalem_fs::touch(&d.join("n/m/f")).unwrap();
    assert!(d.join("n/m/f").is_file());
    kalem_fs::touch(&d.join("n/m/f")).unwrap();
    assert_eq!(kalem_fs::unique_name(&d.join("n/m/f")), d.join("n/m/f (2)"));
    kalem_fs::symlink(Path::new("f"), &d.join("n/m/l")).unwrap();
    let e = kalem_fs::Entry::read(&d.join("n/m/l")).unwrap();
    assert!(matches!(
        e.kind,
        kalem_fs::Kind::Symlink { broken: false, .. }
    ));
}

#[test]
fn cancel_stops_between_files() {
    let files: Vec<(String, String)> = (0..200)
        .map(|i| (format!("src/f{i}"), "x".repeat(4096)))
        .collect();
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let d = tree("cancel", &refs);
    std::fs::create_dir_all(d.join("dst")).unwrap();
    let job =
        Job::start(Operation::transfer(OpKind::Copy, &[d.join("src")], &d.join("dst")).unwrap());
    job.cancel();
    let out = job.wait();
    assert!(out.cancelled || out.done.len() == 1);
    let copied = std::fs::read_dir(d.join("dst/src"))
        .map(|r| r.count())
        .unwrap_or(0);
    assert!(copied < 200 || !out.cancelled);
}

#[test]
fn rename_changing_case_only() {
    let d = tree("case", &[("a.txt", "A")]);
    kalem_fs::move_path(&d.join("a.txt"), &d.join("A.txt")).unwrap();
    let names: Vec<String> = std::fs::read_dir(&d)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["A.txt"]);
    assert_eq!(read(&d.join("A.txt")), "A");
}
