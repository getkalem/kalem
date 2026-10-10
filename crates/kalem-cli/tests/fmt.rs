//! `kalem fmt` on code: the formatter set for the file's type in
//! `settings.toml` (`[formatters]`), the text on its standard input.
#![cfg(unix)]

use std::process::ExitCode;

#[test]
fn code_is_formatted_by_the_type_s_formatter() {
    let dir = std::env::temp_dir().join(format!("kalem-fmt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("config")).unwrap();
    std::fs::write(
        dir.join("config/settings.toml"),
        "[formatters]\nfoo = \"tr a-z A-Z\"\nbar = \"off\"\n",
    )
    .unwrap();
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("KALEM_CONFIG_DIR", dir.join("config"));
        std::env::set_var("KALEM_STATE_DIR", dir.join("state"));
    }
    let foo = dir.join("a.foo");
    std::fs::write(&foo, "hello\n").unwrap();
    let run = |args: &[&std::ffi::OsStr]| {
        let mut all: Vec<&std::ffi::OsStr> = vec!["kalem".as_ref(), "fmt".as_ref()];
        all.extend_from_slice(args);
        kalem_cli::run(all)
    };
    // `--check` names the file that would change and fails.
    assert_eq!(
        run(&["--check".as_ref(), foo.as_os_str()]),
        ExitCode::FAILURE
    );
    assert_eq!(std::fs::read_to_string(&foo).unwrap(), "hello\n");
    // Formatted in place, and then up to date.
    assert_eq!(run(&[foo.as_os_str()]), ExitCode::SUCCESS);
    assert_eq!(std::fs::read_to_string(&foo).unwrap(), "HELLO\n");
    assert_eq!(
        run(&["--check".as_ref(), foo.as_os_str()]),
        ExitCode::SUCCESS
    );
    // A type turned off is left as it is, not an error.
    let bar = dir.join("a.bar");
    std::fs::write(&bar, "hello\n").unwrap();
    assert_eq!(run(&[bar.as_os_str()]), ExitCode::SUCCESS);
    assert_eq!(std::fs::read_to_string(&bar).unwrap(), "hello\n");
    let _ = std::fs::remove_dir_all(&dir);
}
