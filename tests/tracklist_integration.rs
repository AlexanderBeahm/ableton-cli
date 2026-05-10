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
fn tracklist_against_real_als_starts_at_zero_and_has_total() {
    let mut cmd = Command::cargo_bin("ableton-cli").unwrap();
    cmd.arg("tracklist").arg(fixture_path("forjc.als"));
    cmd.assert()
        .success()
        .stdout(predicate::str::starts_with("1. 00:00:000 - "))
        .stdout(predicate::str::contains("\nTotal Length: "));
}

#[test]
fn tracklist_against_real_als_dedupes_to_expected_count() {
    let output = Command::cargo_bin("ableton-cli")
        .unwrap()
        .arg("tracklist")
        .arg(fixture_path("forjc.als"))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output).unwrap();
    let entry_lines = text
        .lines()
        .filter(|l| {
            l.split_once('.')
                .is_some_and(|(num, _)| num.trim().parse::<u32>().is_ok())
        })
        .count();
    assert_eq!(entry_lines, 17, "expected 17 unique tracks in forjc.als");
}

#[test]
fn tracklist_writes_to_output_file_when_o_flag_set() {
    let dir = TempDir::new().unwrap();
    let out_path = dir.path().join("tracklist.txt");
    Command::cargo_bin("ableton-cli")
        .unwrap()
        .arg("tracklist")
        .arg(fixture_path("forjc.als"))
        .arg("-o")
        .arg(&out_path)
        .assert()
        .success()
        .stdout(predicate::str::is_empty());

    let written = std::fs::read_to_string(&out_path).unwrap();
    assert!(written.starts_with("1. 00:00:000 - "));
    assert!(written.contains("\nTotal Length: "));
}

#[test]
fn tracklist_full_paths_flag_emits_absolute_paths() {
    let output = Command::cargo_bin("ableton-cli")
        .unwrap()
        .args([
            "tracklist",
            fixture_path("forjc.als").to_str().unwrap(),
            "--full-paths",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output).unwrap();
    // forjc.als references absolute Windows paths under C:/Users/...
    assert!(
        text.contains("C:/"),
        "expected absolute path in output, got:\n{text}"
    );
}

#[test]
fn errors_clearly_when_path_does_not_exist() {
    Command::cargo_bin("ableton-cli")
        .unwrap()
        .args(["tracklist", "/no/such/path.als"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("path does not exist"));
}

#[test]
fn errors_when_file_is_not_an_als() {
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("notes.txt");
    std::fs::write(&p, "hi").unwrap();
    Command::cargo_bin("ableton-cli")
        .unwrap()
        .arg("tracklist")
        .arg(&p)
        .assert()
        .failure()
        .stderr(predicate::str::contains("expected an .als file"));
}

#[test]
fn errors_when_folder_has_no_als() {
    let dir = TempDir::new().unwrap();
    Command::cargo_bin("ableton-cli")
        .unwrap()
        .arg("tracklist")
        .arg(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("no .als file"));
}

#[test]
fn folder_target_resolves_single_als() {
    let dir = TempDir::new().unwrap();
    let dest = dir.path().join("forjc.als");
    std::fs::copy(fixture_path("forjc.als"), &dest).unwrap();
    Command::cargo_bin("ableton-cli")
        .unwrap()
        .arg("tracklist")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::starts_with("1. "));
}

#[test]
fn folder_target_errors_with_multiple_als() {
    let dir = TempDir::new().unwrap();
    std::fs::copy(fixture_path("forjc.als"), dir.path().join("a.als")).unwrap();
    std::fs::copy(fixture_path("forjc.als"), dir.path().join("b.als")).unwrap();
    Command::cargo_bin("ableton-cli")
        .unwrap()
        .arg("tracklist")
        .arg(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("multiple .als"));
}

#[test]
fn help_flag_lists_subcommand() {
    Command::cargo_bin("ableton-cli")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("tracklist"));
}

#[test]
fn tracklist_help_documents_flags() {
    Command::cargo_bin("ableton-cli")
        .unwrap()
        .args(["tracklist", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--output"))
        .stdout(predicate::str::contains("--full-paths"));
}
