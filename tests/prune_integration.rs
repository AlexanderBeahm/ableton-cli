use std::path::PathBuf;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn prune_dry_run_lists_orphan_without_deleting() {
    let dir = TempDir::new().unwrap();
    let als = dir.path().join("forjc.als");
    std::fs::copy(fixture_path("forjc.als"), &als).unwrap();
    let orphan = dir.path().join("orphan.wav");
    std::fs::write(&orphan, b"junk").unwrap();

    Command::cargo_bin("ableton-cli")
        .unwrap()
        .arg("prune")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Unreferenced files:"))
        .stdout(predicate::str::contains("orphan.wav"))
        .stdout(predicate::str::contains("Run with --delete"));

    assert!(orphan.exists(), "dry-run must not delete files");
    assert!(als.exists(), ".als file must remain after dry-run");
}

#[test]
fn prune_with_delete_removes_orphan_only() {
    let dir = TempDir::new().unwrap();
    let als = dir.path().join("forjc.als");
    std::fs::copy(fixture_path("forjc.als"), &als).unwrap();
    let orphan = dir.path().join("orphan.wav");
    std::fs::write(&orphan, b"junk").unwrap();

    Command::cargo_bin("ableton-cli")
        .unwrap()
        .args(["prune"])
        .arg(dir.path())
        .arg("--delete")
        .assert()
        .success()
        .stdout(predicate::str::contains("Deleted"))
        .stdout(predicate::str::contains("orphan.wav"));

    assert!(!orphan.exists(), "--delete must remove orphan.wav");
    assert!(als.exists(), ".als file must remain");
}

#[test]
fn prune_writes_to_output_file_when_o_flag_set() {
    let dir = TempDir::new().unwrap();
    std::fs::copy(fixture_path("forjc.als"), dir.path().join("forjc.als")).unwrap();
    std::fs::write(dir.path().join("orphan.wav"), b"junk").unwrap();

    let out_dir = TempDir::new().unwrap();
    let out_path = out_dir.path().join("report.txt");
    Command::cargo_bin("ableton-cli")
        .unwrap()
        .arg("prune")
        .arg(dir.path())
        .arg("-o")
        .arg(&out_path)
        .assert()
        .success()
        .stdout(predicate::str::is_empty());

    let written = std::fs::read_to_string(&out_path).unwrap();
    assert!(written.contains("orphan.wav"));
}

#[test]
fn prune_reports_no_files_when_clean() {
    let dir = TempDir::new().unwrap();
    std::fs::copy(fixture_path("forjc.als"), dir.path().join("forjc.als")).unwrap();

    Command::cargo_bin("ableton-cli")
        .unwrap()
        .arg("prune")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("No unreferenced audio files"));
}

#[test]
fn prune_ignores_non_audio_files() {
    let dir = TempDir::new().unwrap();
    std::fs::copy(fixture_path("forjc.als"), dir.path().join("forjc.als")).unwrap();
    std::fs::write(dir.path().join("notes.txt"), b"my notes").unwrap();
    std::fs::write(dir.path().join("orphan.wav"), b"junk").unwrap();

    let output = Command::cargo_bin("ableton-cli")
        .unwrap()
        .arg("prune")
        .arg(dir.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("orphan.wav"));
    assert!(!text.contains("notes.txt"));
}

#[test]
fn prune_skips_backup_directory() {
    let dir = TempDir::new().unwrap();
    std::fs::copy(fixture_path("forjc.als"), dir.path().join("forjc.als")).unwrap();
    let backup = dir.path().join("Backup");
    std::fs::create_dir(&backup).unwrap();
    let in_backup = backup.join("snapshot.wav");
    std::fs::write(&in_backup, b"old").unwrap();

    Command::cargo_bin("ableton-cli")
        .unwrap()
        .arg("prune")
        .arg(dir.path())
        .arg("--delete")
        .assert()
        .success();

    assert!(
        in_backup.exists(),
        "files in Backup/ must never be deleted"
    );
}

#[test]
fn prune_help_documents_flags() {
    Command::cargo_bin("ableton-cli")
        .unwrap()
        .args(["prune", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--delete"))
        .stdout(predicate::str::contains("--output"));
}
