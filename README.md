# ableton-cli

A Rust command-line utility for inspecting and transforming Ableton Live
projects (`.als`).

`.als` files are gzipped XML; this CLI decompresses, parses, and walks the
project model so you can extract information about it without opening Ableton.

Much of project focus as of v0.1.0 is on DJ mixes, samples, and mixtapes.
More broad functionality may be supported in future releases.

## Status

- **Live version:** Live 11 project files.
- **Commands:** `tracklist`, `prune`. The internals are organized so future
  commands (e.g. transposition, sample inventory) reuse the same loader,
  parser, and data model.

## Installation

### Build from source (Linux / WSL)

```bash
cargo build --release
# binary at target/release/ableton-cli
```

### Build from source (Windows)

Native Windows build (recommended):

```powershell
cargo build --release
# binary at target\release\ableton-cli.exe
```

Cross-compile from Linux/WSL using mingw:

```bash
sudo apt install mingw-w64
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
# binary at target/x86_64-pc-windows-gnu/release/ableton-cli.exe
```

The repository's `.cargo/config.toml` points at `x86_64-w64-mingw32-gcc` for
the `gnu` target, so no further configuration is required.

### Add to PATH

Drop the built binary somewhere on your `PATH` (e.g. `~/.local/bin` on Linux,
`%USERPROFILE%\bin` on Windows added to the user `Path` environment variable).

## Usage

```bash
ableton-cli --help
ableton-cli <COMMAND> --help
```

### `tracklist`

Build a deduplicated tracklist of audio samples used in arrangement view,
sorted by their first occurrence on the timeline.

```bash
# Print to stdout (default format: "{ARTIST} - {TITLE}")
ableton-cli tracklist /path/to/project.als

# Or pass the project folder (must contain exactly one .als)
ableton-cli tracklist "/path/to/My Song Project"

# Write to file
ableton-cli tracklist /path/to/project.als -o tracklist.txt

# Use absolute sample paths in the output
ableton-cli tracklist /path/to/project.als --full-paths

# Custom label template
ableton-cli tracklist /path/to/project.als --track-template "{TITLE} ({YEAR})"
```

Output format:

```
1. 00:00:000 - Alice - Track One
2. 00:01:300 - Bob - Track Two
3. 00:03:236 - some-untagged-file

Total Length: 00:06:303
```

Timestamps use `MM:SS:mmm` (minutes / seconds / milliseconds). Tempo
automation is honoured: clip beat positions are converted to wall-clock
seconds by integrating the project's tempo curve.

### Labels and `--track-template`

By default each entry is labeled `{ARTIST} - {TITLE}`, reading the
metadata embedded in the audio file (ID3, Vorbis comments, MP4 atoms,
etc.). When the metadata is absent or the file isn't reachable on disk,
the label falls back silently to the filename with the extension stripped.

`--track-template <STRING>` lets you customize the format. Supported
tokens (case-insensitive):

| Token            | Source                                                   |
|------------------|----------------------------------------------------------|
| `{ARTIST}`       | artist tag                                               |
| `{TITLE}`        | title tag                                                |
| `{ALBUM}`        | album tag                                                |
| `{ALBUMARTIST}`  | album artist tag                                         |
| `{YEAR}`         | recording year                                           |
| `{TRACK}`        | track number (decimal, unpadded)                         |
| `{GENRE}`        | genre tag                                                |
| `{COMPOSER}`     | composer tag                                             |
| `{COMMENT}`      | comment tag                                              |
| `{FILENAME}`     | basename of the sample's path, extension stripped        |
| `{PATH}`         | absolute path (or relative path, if no absolute is known)|

Unknown tokens (e.g. `{FOO}`) render as empty. If a token is missing,
surrounding separator characters around it are cleaned up so you don't
get stray punctuation. For example, with a file tagged only with artist
`Alice`:

- `"{ARTIST} - {TITLE}"` → `Alice` (trailing ` - ` stripped)
- `"{ALBUM} | {ARTIST} - {TITLE}"` → `Alice` (leading and trailing
  separators stripped)
- `"{ARTIST} - {ALBUM} - {TITLE}"` → `Alice` (full collapse — no title,
  no album, only artist remains)

Separator characters used for cleanup: space, tab, `-`, `–`, `—`, `,`,
`|`, `;`. Path-like characters (`/`, `\`, `:`) are deliberately not
treated as separators so paths and time-of-day strings survive.

If every token resolves to empty, the label falls back to the filename
with the extension stripped.

`--full-paths` is exactly equivalent to `--track-template "{PATH}"`.
The two flags cannot be combined.

### Behaviour

- Only **arrangement-view audio clips** are considered (session view and MIDI
  clips are ignored).
- Samples are deduplicated by absolute path, then relative path, then
  display name. The first occurrence wins.
- **Total length** is the latest end position across **all** audio clips on
  any track, including duplicates.
- When given a folder, the CLI looks for exactly one `.als` (non-recursive)
  and errors otherwise.
- Metadata reads are best-effort: missing or unreadable files silently
  fall back to filenames.

### `prune`

List (and optionally delete) audio files inside the project directory that
are not referenced by the `.als`. Useful for cleaning up samples left
behind after iterating on a project.

```bash
# Dry-run: report unreferenced files, don't delete anything.
ableton-cli prune "/path/to/My Song Project"

# Actually delete the listed files.
ableton-cli prune "/path/to/My Song Project" --delete

# Write the report to a file instead of stdout.
ableton-cli prune /path/to/project.als -o prune-report.txt
```

Behaviour:

- **Dry-run by default.** Pass `--delete` to actually remove files.
- The walk considers **every** `<SampleRef>` in the project XML — not just
  arrangement clips. Samples loaded into instruments (Sampler, Simpler,
  Impulse, drum racks) and convolution IRs are treated as referenced.
- Candidate file extensions: `.wav`, `.aiff`, `.aif`, `.flac`, `.mp3`,
  `.ogg`, `.m4a`. Other file types are never touched.
- Only files **inside the project directory** are inspected. References to
  files in user libraries or other absolute paths are ignored.
- Ableton-owned subdirectories (`Backup/`, `Ableton Project Info/`) are
  skipped.
- `.asd` analysis sidecars are kept when their sibling audio is referenced
  and pruned when it isn't.

## Development

```bash
cargo test                    # run all tests
cargo llvm-cov --summary-only # measure coverage (requires cargo-llvm-cov)
```

Coverage target: at least 85% line coverage over the whole crate.

## Releases

See [`HISTORY.md`](./HISTORY.md) for user-facing changes per version.
Releases are produced automatically when `Cargo.toml` `version` is bumped
on `main`; each version must have a matching entry in `HISTORY.md`.
