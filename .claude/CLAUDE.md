# ableton-cli — context for future Claude sessions

A Rust CLI utility for inspecting Ableton Live 11 projects (`.als` files,
which are gzipped XML). The tool decompresses, parses, and walks the project
model so users can extract information without opening Ableton.

The codebase is structured so additional commands (transposition, sample
inventory, project diffing, etc.) can be added without rewriting the loader
or parser.

---

## Quick reference

```bash
cargo build --release                   # → target/release/ableton-cli
cargo test                              # full test suite (unit + integration)
cargo llvm-cov --summary-only           # coverage report (target ≥ 85%)

# Run the only command currently implemented:
ableton-cli tracklist <PATH> [-o FILE] [--full-paths]
```

`<PATH>` is either an `.als` file or a folder containing exactly one `.als`.

---

## Architecture

```
src/
├── main.rs               thin: ExitCode + ableton_cli::run()
├── lib.rs                module root, public run()
├── error.rs              thiserror enum (Error) + Result alias
├── cli.rs                clap derive definitions
├── output.rs             WriterTarget: Stdout | File(PathBuf)
├── time.rs               Tempo + AutomatedTempo + integration math
├── project/
│   ├── mod.rs            Project, AudioTrack, AudioClip, SampleRef
│   ├── loader.rs         path resolution + gunzip → XML string
│   └── parser.rs         XML → typed model (roxmltree)
└── commands/
    ├── mod.rs            command modules
    └── tracklist.rs      tracklist logic + formatter

tests/
├── fixtures/forjc.als    real 37 KB Live 11 fixture (3 audio tracks,
│                         tempo automation 132→135→132→155 BPM)
└── tracklist_integration.rs  end-to-end binary tests via assert_cmd
```

### Data flow for a command

1. `cli::Cli::execute()` dispatches the parsed subcommand.
2. The command calls `project::loader::resolve_als_path()` → resolves a
   user-supplied path to a concrete `.als` file (file or folder).
3. `project::loader::read_als_xml()` → opens the file, gunzips it,
   returns the raw XML string.
4. `project::parser::parse()` → returns a typed `Project`.
5. The command consumes the model (e.g. `commands::tracklist::build_tracklist`).
6. Output is sent through `output::WriterTarget` (stdout or a file).

Loader, parser, and model are deliberately command-agnostic — new commands
should reuse them.

---

## Data model essentials

```rust
struct Project { tempo: Tempo, audio_tracks: Vec<AudioTrack> }
struct AudioTrack { id: String, name: String, clips: Vec<AudioClip> }
struct AudioClip { name, start_beats: f64, end_beats: f64, sample: SampleRef }
struct SampleRef { name, absolute_path: Option<PathBuf>, relative_path: Option<PathBuf> }

enum Tempo {
    Constant(f64),                                    // BPM
    Automated(AutomatedTempo),                        // piecewise-linear curve
}
```

- **Beats are the canonical time unit on disk.** All clip start/end positions
  in `.als` are stored in beats. Use `Tempo::seconds_at(beats)` to convert.
- **`SampleRef::identity()`** is the dedup key (absolute path → relative path
  → name).
- **`SampleRef::display_label(full_paths)`** picks the right rendering for
  output: basename by default, absolute path when the flag is set.
- **`Project::last_clip_end_beats()`** returns the latest `end_beats` across
  all audio tracks — used for project total length.

---

## Ableton XML notes (the non-obvious bits)

These details came from inspecting a real Live 11.3.43 project. They are not
guaranteed to hold for other Live versions.

### Arrangement-view audio clips

Path from `<LiveSet>` to a single arrangement-view audio clip:

```
LiveSet
└── Tracks
    └── AudioTrack
        └── DeviceChain
            └── MainSequencer
                └── Sample
                    └── ArrangerAutomation
                        └── Events
                            └── AudioClip
                                ├── CurrentStart Value="…"  (beats)
                                ├── CurrentEnd   Value="…"  (beats)
                                ├── Name         Value="…"  (display name)
                                └── SampleRef
                                    └── FileRef
                                        ├── Path         Value="…"  (absolute)
                                        ├── RelativePath Value="…"
                                        └── Name         Value="…"
```

Session-view clips live elsewhere (`ClipSlotList`) and are intentionally
ignored — `tracklist` is arrangement-only.

### Tempo

Base BPM:

```
LiveSet/MasterTrack/DeviceChain/Mixer/Tempo/Manual @Value="120"
```

That `Tempo` element also has an `<AutomationTarget Id="N">` child. To
resolve tempo automation, find an `<AutomationEnvelope>` under
`LiveSet/MasterTrack/AutomationEnvelopes/Envelopes` whose
`EnvelopeTarget/PointeeId @Value` equals that `N`. Its
`Automation/Events/FloatEvent` children describe the curve.

Two non-obvious things about the FloatEvents:

1. **Negative-beat sentinel events** (e.g. `Time="-63072000"`) represent the
   "initial value before the song starts". Treat the latest negative-beat
   event as the BPM at beat 0; drop the rest.
2. **Multiple events at the same beat** can occur from envelope edits.
   `Tempo::from_automation_events` keeps the last one (input order).

If no automation envelope targets the tempo, the parser falls back to the
constant `<Manual>` value.

### Beat → time conversion (the math)

Tempo interpolates linearly with beats between control points. The closed
form for time elapsed across one segment `(b₁, t₁) → (b₂, t₂)`:

```
if t₁ == t₂:    Δt = (b₂ - b₁) · 60 / t₁
else:           Δt = 60 · (b₂ - b₁) / (t₂ - t₁) · ln(t₂ / t₁)
```

This comes from `dt/db = 60/T(b)` where `T(b)` is linear in `b`. Beats past
the final control point hold the last BPM (extrapolation, not loop).

Cumulative segment times are precomputed in `AutomatedTempo::cumulative_seconds`,
making `seconds_at` O(log n) via binary search over points.

---

## Tracklist semantics (current command)

`build_tracklist(project, full_paths)` →

1. Collect every `AudioClip` from every `AudioTrack`.
2. Sort by `start_beats` ascending.
3. Walk in order, deduplicate by `SampleRef::identity()` (first occurrence wins).
4. For each kept entry: convert `start_beats` → seconds via the project tempo.
5. **Total length** = `tempo.seconds_at(project.last_clip_end_beats())` —
   uses **all** clips (including the duplicates not in the entry list).

Output format:

```
1. 00:00:000 - sample-a.flac
2. 03:52:727 - sample-b.flac
…
N. MM:SS:mmm - sample-x.ext

Total Length: MM:SS:mmm
```

Minutes can grow past 99 (e.g. `100:01:005`).

---

## Adding a new command (extension recipe)

1. Define an args struct + `Command` variant in `src/cli.rs`. clap will
   wire `--help` and parsing automatically.
2. Add a module under `src/commands/` (e.g. `src/commands/foo.rs`) and
   register it in `src/commands/mod.rs`.
3. The command should:
   - Take a typed options struct (mirror `TracklistOptions`).
   - Resolve the project via `project::loader` + `project::parser`.
   - Operate on the typed `Project` model — do not re-traverse XML.
   - Emit text via `output::WriterTarget` (stdout or `-o`).
4. Add unit tests beside the new module and an integration test under
   `tests/` if the command's surface is non-trivial.
5. Update README.md usage section.

If a command needs project data the parser doesn't currently expose
(e.g. MIDI clips, plugin instances), extend the parser and model — keep
parsing centralized.

---

## Testing & coverage

- **Unit tests** colocate in each module under `#[cfg(test)] mod tests`.
- **Integration tests** under `tests/` exercise the binary via
  `assert_cmd::Command::cargo_bin("ableton-cli")`.
- `tests/fixtures/forjc.als` is a real Live 11 project copied from a user
  workstation. It exercises tempo automation and the dedup logic. **Do not
  delete or modify it without checking the integration tests** — several
  tests assert on its content (e.g. 17 unique tracks, "C:/" in absolute
  paths).
- Coverage tool: `cargo-llvm-cov` (the v0.6.x line works on rustc 1.86;
  v0.8+ requires rustc 1.87+).
- Coverage target: 85% line. CI enforces this via
  `cargo llvm-cov --fail-under-lines 85` in `.github/workflows/ci.yml`.

Run a focused failing test with `cargo test <pattern>`.

---

## Build / release

- **Linux/WSL native:** `cargo build --release`.
- **Windows native:** `cargo build --release` on a Windows host (MSVC).
- **Windows from Linux/WSL:**
  `sudo apt install mingw-w64 && rustup target add x86_64-pc-windows-gnu`,
  then `cargo build --release --target x86_64-pc-windows-gnu`.
  `.cargo/config.toml` already points at the mingw linker.
- **CI:** `.github/workflows/ci.yml` runs build + tests + coverage on every
  push/PR. `.github/workflows/release.yml` cross-builds Linux + Windows
  (MSVC) artifacts on tags `v*`.

---

## Conventions in this codebase

- Errors flow through one crate-wide `Error` enum (`src/error.rs`); commands
  return `anyhow::Result` to layer additional context cheaply.
- The lib (`ableton_cli`) holds all logic; `main.rs` only owns the process
  exit code. Tests target the lib for fast iteration and the binary for
  end-to-end CLI behaviour.
- XML traversal uses `roxmltree` (read-only, allocation-light). Helpers
  `child(node, "Name")` and `child_value_attr(node, "Child", "Attr")` keep
  the parser readable; prefer them over chained `descendants().find()`.
- Path-typed values: paths from `.als` (which are Windows-style on Windows
  projects) round-trip as `PathBuf`. Don't convert to strings until display
  time.
- Do not add commands that require Ableton to be installed; everything must
  run from the `.als` file alone.

---

## Things this project intentionally does **not** handle

- Live versions other than 11 (schema may differ — needs verification).
- Session-view clips.
- MIDI clips.
- Tempo automation curves with non-linear segment shapes (Live exposes a
  shape per point in some versions; we treat all as linear).
- Project rendering, clip auditioning, or anything requiring the audio
  files referenced by `<FileRef>`.
