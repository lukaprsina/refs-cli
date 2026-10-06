//! `atomic::write`: whole-file writes a concurrent reader or writer never sees half done.

use std::fs;

use refs_cli::atomic;
use tempfile::TempDir;

#[test]
fn concurrent_writers_each_succeed_and_leave_one_whole_text() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("refs.lock");
    let texts: Vec<String> = (0..8).map(|i| format!("{i}\n").repeat(200_000)).collect();

    std::thread::scope(|scope| {
        let writers: Vec<_> = texts
            .iter()
            .map(|text| scope.spawn(|| atomic::write(&path, text)))
            .collect();
        for writer in writers {
            writer.join().unwrap().expect("every write succeeds");
        }
    });

    let written = fs::read_to_string(&path).unwrap();
    assert!(
        texts.contains(&written),
        "the file is one writer's whole text"
    );
    let names: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["refs.lock"], "no temp file is left behind");
}

#[cfg(unix)]
#[test]
fn a_new_file_gets_the_mode_a_plain_create_would() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let plain = dir.path().join("plain");
    let atomic_file = dir.path().join("atomic");
    fs::write(&plain, "x").unwrap();

    atomic::write(&atomic_file, "x").unwrap();

    let mode = |p: &std::path::Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&atomic_file), mode(&plain));
}

#[cfg(unix)]
#[test]
fn an_existing_files_mode_carries_over() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let path = dir.path().join("script");
    fs::write(&path, "old").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o750)).unwrap();

    atomic::write(&path, "new").unwrap();

    assert_eq!(fs::read_to_string(&path).unwrap(), "new");
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o750
    );
}
