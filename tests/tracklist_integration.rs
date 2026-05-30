use std::io::Write;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use flate2::Compression;
use flate2::write::GzEncoder;
use id3::TagLike;
use predicates::prelude::*;
use tempfile::TempDir;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// Write a minimal but valid RIFF/WAVE file (PCM mono 8000Hz 8-bit, two
/// silent samples) at `path`.
fn write_minimal_wav(path: &Path) {
    // fmt chunk body
    let fmt: [u8; 16] = [
        0x01, 0x00, // PCM
        0x01, 0x00, // 1 channel
        0x40, 0x1F, 0x00, 0x00, // 8000 Hz
        0x40, 0x1F, 0x00, 0x00, // byte rate = 8000
        0x01, 0x00, // block align
        0x08, 0x00, // 8 bits/sample
    ];
    let data: [u8; 2] = [0x80, 0x80]; // two silent 8-bit samples

    let mut riff_body = Vec::new();
    riff_body.extend_from_slice(b"WAVE");
    riff_body.extend_from_slice(b"fmt ");
    riff_body.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
    riff_body.extend_from_slice(&fmt);
    riff_body.extend_from_slice(b"data");
    riff_body.extend_from_slice(&(data.len() as u32).to_le_bytes());
    riff_body.extend_from_slice(&data);

    let mut file = Vec::new();
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&(riff_body.len() as u32).to_le_bytes());
    file.extend_from_slice(&riff_body);

    std::fs::write(path, file).expect("write minimal wav");
}

/// Write a WAV file with the given ID3 artist/title (and optionally year) tags.
fn write_tagged_wav(path: &Path, artist: &str, title: &str, year: Option<i32>) {
    write_minimal_wav(path);
    let mut tag = id3::Tag::new();
    tag.set_artist(artist);
    tag.set_title(title);
    if let Some(y) = year {
        tag.set_year(y);
    }
    tag.write_to_wav_path(path, id3::Version::Id3v24)
        .expect("write id3 tag");
}

/// Build a minimal Ableton `.als` (gzipped XML) at `als_path` that references
/// each `(absolute_path, basename)` tuple as a separate arrangement-view
/// audio clip. Clip start beats increment by 16 so the timeline order
/// matches input order.
fn write_als_referencing(als_path: &Path, samples: &[(&Path, &str)]) {
    let mut clips_xml = String::new();
    for (i, (abs_path, name)) in samples.iter().enumerate() {
        let start = i as u32 * 16;
        let end = start + 8;
        let abs = abs_path.to_str().expect("utf-8 path");
        clips_xml.push_str(&format!(
            r#"
              <AudioClip Id="{i}" Time="{start}">
                <CurrentStart Value="{start}"/>
                <CurrentEnd Value="{end}"/>
                <Name Value="{name}"/>
                <SampleRef>
                  <FileRef>
                    <Path Value="{abs}"/>
                    <Name Value="{name}"/>
                  </FileRef>
                </SampleRef>
              </AudioClip>"#
        ));
    }
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Ableton>
  <LiveSet>
    <Tracks>
      <AudioTrack Id="1">
        <Name><EffectiveName Value="Test Track"/></Name>
        <DeviceChain><MainSequencer><Sample><ArrangerAutomation><Events>{clips_xml}
            </Events></ArrangerAutomation></Sample></MainSequencer></DeviceChain>
      </AudioTrack>
    </Tracks>
    <MasterTrack>
      <DeviceChain><Mixer><Tempo><Manual Value="120"/></Tempo></Mixer></DeviceChain>
    </MasterTrack>
  </LiveSet>
</Ableton>"#
    );
    let file = std::fs::File::create(als_path).expect("create als");
    let mut encoder = GzEncoder::new(file, Compression::default());
    encoder.write_all(xml.as_bytes()).expect("gzip xml");
    encoder.finish().expect("flush gzip");
}

struct TaggedProject {
    _dir: TempDir,
    als: PathBuf,
    tagged_a: PathBuf,
    tagged_b: PathBuf,
    untagged: PathBuf,
}

fn build_tagged_project() -> TaggedProject {
    let dir = TempDir::new().unwrap();
    let tagged_a = dir.path().join("tagged_a.wav");
    let tagged_b = dir.path().join("tagged_b.wav");
    let untagged = dir.path().join("untagged.wav");

    write_tagged_wav(&tagged_a, "Alice", "Track One", Some(2024));
    write_tagged_wav(&tagged_b, "Bob", "Track Two", None);
    write_minimal_wav(&untagged);

    let als = dir.path().join("tagged.als");
    write_als_referencing(
        &als,
        &[
            (&tagged_a, "tagged_a.wav"),
            (&tagged_b, "tagged_b.wav"),
            (&untagged, "untagged.wav"),
        ],
    );

    TaggedProject {
        _dir: dir,
        als,
        tagged_a,
        tagged_b,
        untagged,
    }
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
        .stdout(predicate::str::contains("--full-paths"))
        .stdout(predicate::str::contains("--track-template"));
}

#[test]
fn tracklist_default_uses_artist_title_from_metadata() {
    let proj = build_tagged_project();
    let output = Command::cargo_bin("ableton-cli")
        .unwrap()
        .arg("tracklist")
        .arg(&proj.als)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output).unwrap();
    assert!(
        text.contains("Alice - Track One"),
        "expected tagged label, got:\n{text}"
    );
    assert!(
        text.contains("Bob - Track Two"),
        "expected tagged label, got:\n{text}"
    );
    // Untagged WAV falls back to filename without extension.
    assert!(
        text.contains("- untagged\n"),
        "expected filename fallback, got:\n{text}"
    );
}

#[test]
fn tracklist_track_template_renders_title_only() {
    let proj = build_tagged_project();
    let output = Command::cargo_bin("ableton-cli")
        .unwrap()
        .args(["tracklist"])
        .arg(&proj.als)
        .args(["--track-template", "{TITLE}"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("- Track One\n"), "got:\n{text}");
    assert!(text.contains("- Track Two\n"), "got:\n{text}");
    // Untagged falls back to filename when the template resolves empty.
    assert!(text.contains("- untagged\n"), "got:\n{text}");
}

#[test]
fn tracklist_track_template_collapses_missing_album() {
    let proj = build_tagged_project();
    let output = Command::cargo_bin("ableton-cli")
        .unwrap()
        .args(["tracklist"])
        .arg(&proj.als)
        .args(["--track-template", "{ARTIST} | {ALBUM} | {TITLE}"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output).unwrap();
    // Neither fixture has an album tag — the middle field collapses, leaving
    // one separator between artist and title.
    assert!(
        text.contains("Alice | Track One"),
        "expected collapsed separators, got:\n{text}"
    );
    assert!(
        text.contains("Bob | Track Two"),
        "expected collapsed separators, got:\n{text}"
    );
}

#[test]
fn tracklist_track_template_with_only_unknown_tokens_falls_back_to_filename() {
    let proj = build_tagged_project();
    let output = Command::cargo_bin("ableton-cli")
        .unwrap()
        .args(["tracklist"])
        .arg(&proj.als)
        .args(["--track-template", "{NOPE} - {ALSO_NOPE}"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("- tagged_a\n"), "got:\n{text}");
    assert!(text.contains("- tagged_b\n"), "got:\n{text}");
    assert!(text.contains("- untagged\n"), "got:\n{text}");
}

#[test]
fn tracklist_full_paths_and_track_template_conflict() {
    Command::cargo_bin("ableton-cli")
        .unwrap()
        .args([
            "tracklist",
            "/tmp/anything.als",
            "--full-paths",
            "--track-template",
            "{TITLE}",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("cannot be used with")
                .or(predicate::str::contains("conflict"))
                .or(predicate::str::contains("cannot be used")),
        );
}

#[test]
fn tracklist_full_paths_emits_absolute_paths_for_tagged_project() {
    let proj = build_tagged_project();
    let output = Command::cargo_bin("ableton-cli")
        .unwrap()
        .arg("tracklist")
        .arg(&proj.als)
        .arg("--full-paths")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output).unwrap();
    assert!(
        text.contains(proj.tagged_a.to_str().unwrap()),
        "expected absolute path for tagged_a"
    );
    assert!(
        text.contains(proj.tagged_b.to_str().unwrap()),
        "expected absolute path for tagged_b"
    );
    assert!(
        text.contains(proj.untagged.to_str().unwrap()),
        "expected absolute path for untagged"
    );
}
