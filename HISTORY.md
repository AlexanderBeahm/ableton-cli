# Release History

User-facing changes per release. Add a new section at the top whenever
`Cargo.toml` `version` is bumped; the release workflow extracts the matching
section as the GitHub Release body.

Section header format: `## vMAJOR.MINOR.PATCH - YYYY-MM-DD`
(the version must match `Cargo.toml` exactly; the workflow keys off it).

## v0.2.0 - 2026-05-12

- **`tracklist` now labels entries with `{ARTIST} - {TITLE}` by default**,
  reading audio metadata (ID3, Vorbis comments, MP4 atoms, WAV/AIFF ID3
  chunks) via the `lofty` crate. Files without metadata or unreadable on
  disk fall back to the filename with the extension stripped. **This
  changes default output** versus v0.1.0, which always showed filenames.
- New `--track-template` flag for custom labels. Supported tokens:
  `{ARTIST}`, `{TITLE}`, `{ALBUM}`, `{ALBUMARTIST}`, `{YEAR}`, `{TRACK}`,
  `{GENRE}`, `{COMPOSER}`, `{COMMENT}`, `{FILENAME}`, `{PATH}`. Missing
  tokens collapse cleanly (separator characters around an empty token are
  trimmed); if every token resolves empty, the filename is used.
- `--full-paths` is now equivalent to `--track-template "{PATH}"`. The
  two flags cannot be combined.

## v0.1.0 - 2026-05-10

- Initial release.
- `tracklist` command: extracts a chronologically-ordered, deduplicated
  list of audio samples used in arrangement view, with timecodes that
  honour tempo automation.
- `prune` command: lists (and optionally deletes) audio files inside a
  project directory that are not referenced by the `.als`.
- Cross-platform builds for Linux (x86_64) and Windows (x86_64 MSVC).
