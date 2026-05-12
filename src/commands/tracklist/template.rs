//! Track-label templates.
//!
//! A template is a small format string with `{TOKEN}` placeholders. Tokens
//! resolve to metadata fields on a `TrackMetadata`; unknown or missing fields
//! resolve to empty. Rendering then walks the substituted output and cleans
//! away separator punctuation that surrounded the empty fields.
//!
//! Cleanup uses a private sentinel character (`\u{0001}`) to mark each empty
//! field at render time. The cleanup pass collapses runs of
//! `[separator chars] [sentinel(s)] [separator chars]` to nothing, then
//! trims leading/trailing separators and collapses repeated whitespace.
//! If the final string is empty, the label falls back to the filename.

use super::metadata::TrackMetadata;

const EMPTY: char = '\u{0001}';

// Note: `/`, `\`, and `:` are intentionally NOT separators — they appear
// inside paths (`{PATH}`) and inside time-of-day or URL-like literals, and
// stripping them would mangle values that contain them.
const SEPARATORS: &[char] = &[' ', '\t', '-', '\u{2013}', '\u{2014}', ',', '|', ';'];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Field {
    Artist,
    Title,
    Album,
    AlbumArtist,
    Year,
    Track,
    Genre,
    Composer,
    Comment,
    Filename,
    Path,
    Unknown,
}

impl Field {
    fn from_name(raw: &str) -> Self {
        match raw.to_ascii_uppercase().as_str() {
            "ARTIST" => Field::Artist,
            "TITLE" => Field::Title,
            "ALBUM" => Field::Album,
            "ALBUMARTIST" => Field::AlbumArtist,
            "YEAR" => Field::Year,
            "TRACK" => Field::Track,
            "GENRE" => Field::Genre,
            "COMPOSER" => Field::Composer,
            "COMMENT" => Field::Comment,
            "FILENAME" => Field::Filename,
            "PATH" => Field::Path,
            _ => Field::Unknown,
        }
    }

    fn resolve(&self, meta: &TrackMetadata) -> Option<String> {
        match self {
            Field::Artist => meta.artist.clone(),
            Field::Title => meta.title.clone(),
            Field::Album => meta.album.clone(),
            Field::AlbumArtist => meta.album_artist.clone(),
            Field::Year => meta.year.clone(),
            Field::Track => meta.track.clone(),
            Field::Genre => meta.genre.clone(),
            Field::Composer => meta.composer.clone(),
            Field::Comment => meta.comment.clone(),
            Field::Filename => Some(meta.filename.clone()),
            Field::Path => meta.path.clone(),
            Field::Unknown => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Literal(String),
    Field(Field),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    tokens: Vec<Token>,
}

impl Template {
    pub fn parse(src: &str) -> Self {
        let mut tokens: Vec<Token> = Vec::new();
        let mut literal = String::new();
        let mut chars = src.chars().peekable();

        while let Some(c) = chars.next() {
            if c != '{' {
                literal.push(c);
                continue;
            }
            // Try to consume a token: '{' [A-Za-z0-9_]+ '}'. Token names
            // must start with a letter to avoid eating literal `{1}`-style
            // text.
            let mut name = String::new();
            let mut first = true;
            while let Some(&next) = chars.peek() {
                let ok = if first {
                    next.is_ascii_alphabetic()
                } else {
                    next.is_ascii_alphanumeric() || next == '_'
                };
                if ok {
                    name.push(next);
                    chars.next();
                    first = false;
                } else {
                    break;
                }
            }
            if name.is_empty() || chars.peek() != Some(&'}') {
                // Not a valid token; treat the '{' (and any letters we ate) as
                // literal characters.
                literal.push('{');
                literal.push_str(&name);
                continue;
            }
            chars.next(); // consume closing '}'
            if !literal.is_empty() {
                tokens.push(Token::Literal(std::mem::take(&mut literal)));
            }
            tokens.push(Token::Field(Field::from_name(&name)));
        }

        if !literal.is_empty() {
            tokens.push(Token::Literal(literal));
        }

        Template { tokens }
    }

    pub fn render(&self, meta: &TrackMetadata) -> String {
        let mut raw = String::new();
        for token in &self.tokens {
            match token {
                Token::Literal(s) => raw.push_str(s),
                Token::Field(f) => match f.resolve(meta) {
                    Some(v) if !v.is_empty() => raw.push_str(&v),
                    _ => raw.push(EMPTY),
                },
            }
        }
        let cleaned = cleanup(&raw);
        if cleaned.is_empty() {
            meta.filename.clone()
        } else {
            cleaned
        }
    }
}

fn is_separator(c: char) -> bool {
    SEPARATORS.contains(&c)
}

fn is_sep_or_empty(c: char) -> bool {
    is_separator(c) || c == EMPTY
}

/// Clean up `raw` so that runs of separator/sentinel characters surrounding
/// empty fields don't leave stray punctuation in the output.
///
/// Rules:
/// - A run of `[separator|sentinel]` chars at the **start** of `raw` is
///   dropped.
/// - A run of `[separator|sentinel]` chars at the **end** of `raw` is
///   dropped.
/// - An **interior** run with at least one sentinel collapses to a single
///   normalized separator: one space, plus the first non-whitespace
///   separator in the run (if any), plus one space.
/// - An **interior** run with no sentinels is preserved verbatim — the user
///   wrote those characters and we honor them.
fn cleanup(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    let n = chars.len();

    // Skip leading separator/sentinel run.
    let mut i = 0;
    while i < n && is_sep_or_empty(chars[i]) {
        i += 1;
    }
    if i >= n {
        return String::new();
    }

    let mut out = String::with_capacity(raw.len());
    while i < n {
        let c = chars[i];
        if !is_sep_or_empty(c) {
            out.push(c);
            i += 1;
            continue;
        }
        // Scan the full separator/sentinel run.
        let mut j = i;
        let mut saw_sentinel = false;
        let mut first_non_space_sep: Option<char> = None;
        while j < n && is_sep_or_empty(chars[j]) {
            if chars[j] == EMPTY {
                saw_sentinel = true;
            } else if chars[j] != ' ' && chars[j] != '\t' && first_non_space_sep.is_none() {
                first_non_space_sep = Some(chars[j]);
            }
            j += 1;
        }
        if j >= n {
            // Trailing run — drop.
            break;
        }
        if saw_sentinel {
            // Interior run with empty field(s): collapse to a single
            // normalized separator.
            out.push(' ');
            if let Some(sep) = first_non_space_sep {
                out.push(sep);
                out.push(' ');
            }
        } else {
            // Interior run with no sentinels — preserve verbatim.
            for c in &chars[i..j] {
                out.push(*c);
            }
        }
        i = j;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta_minimal() -> TrackMetadata {
        TrackMetadata::for_test("song", None)
    }

    fn meta_full() -> TrackMetadata {
        let mut m = TrackMetadata::for_test("song", Some("/proj/song.mp3".into()));
        m.artist = Some("Alice".into());
        m.title = Some("Track One".into());
        m.album = Some("Album X".into());
        m.album_artist = Some("Alice & Friends".into());
        m.year = Some("2024".into());
        m.track = Some("3".into());
        m.genre = Some("House".into());
        m.composer = Some("Alice".into());
        m.comment = Some("note".into());
        m
    }

    #[test]
    fn parses_and_renders_artist_title() {
        let t = Template::parse("{ARTIST} - {TITLE}");
        assert_eq!(t.render(&meta_full()), "Alice - Track One");
    }

    #[test]
    fn renders_with_missing_title_falls_back_to_artist_only() {
        let mut m = meta_full();
        m.title = None;
        let t = Template::parse("{ARTIST} - {TITLE}");
        assert_eq!(t.render(&m), "Alice");
    }

    #[test]
    fn renders_with_no_metadata_falls_back_to_filename() {
        let t = Template::parse("{ARTIST} - {TITLE}");
        assert_eq!(t.render(&meta_minimal()), "song");
    }

    #[test]
    fn renders_leading_separator_after_empty_field_is_stripped() {
        let mut m = meta_full();
        m.album = None;
        let t = Template::parse("{ALBUM} | {ARTIST} - {TITLE}");
        assert_eq!(t.render(&m), "Alice - Track One");
    }

    #[test]
    fn renders_interior_empty_field_collapses() {
        let mut m = meta_full();
        m.album = None;
        let t = Template::parse("{ARTIST} - {ALBUM} - {TITLE}");
        assert_eq!(t.render(&m), "Alice - Track One");
    }

    #[test]
    fn renders_trailing_separator_after_empty_field_is_stripped() {
        let mut m = meta_full();
        m.title = None;
        m.album = None;
        let t = Template::parse("{ARTIST} - {TITLE} - {ALBUM}");
        assert_eq!(t.render(&m), "Alice");
    }

    #[test]
    fn renders_case_insensitive_tokens() {
        let t = Template::parse("{artist} - {Title}");
        assert_eq!(t.render(&meta_full()), "Alice - Track One");
    }

    #[test]
    fn renders_unknown_token_as_empty() {
        let mut m = meta_full();
        m.title = None;
        let t = Template::parse("{ARTIST}{FOO}");
        assert_eq!(t.render(&m), "Alice");
    }

    #[test]
    fn renders_adjacent_tokens_without_separator() {
        let t = Template::parse("{ARTIST}{TITLE}");
        assert_eq!(t.render(&meta_full()), "AliceTrack One");
    }

    #[test]
    fn renders_literal_braces_when_no_valid_token() {
        let t = Template::parse("{notavalidtoken{ARTIST}");
        // `{notavalidtoken` is preserved as a literal; the trailing `{ARTIST}`
        // is a valid token.
        assert_eq!(t.render(&meta_full()), "{notavalidtokenAlice");
    }

    #[test]
    fn renders_unterminated_token_as_literal() {
        let t = Template::parse("hello {ARTIST");
        assert_eq!(t.render(&meta_full()), "hello {ARTIST");
    }

    #[test]
    fn renders_path_token() {
        let t = Template::parse("{PATH}");
        assert_eq!(t.render(&meta_full()), "/proj/song.mp3");
    }

    #[test]
    fn renders_filename_token_always_populated() {
        let t = Template::parse("{FILENAME}");
        assert_eq!(t.render(&meta_full()), "song");
    }

    #[test]
    fn renders_empty_template_falls_back_to_filename() {
        let t = Template::parse("");
        assert_eq!(t.render(&meta_full()), "song");
    }

    #[test]
    fn renders_year_and_track_tokens() {
        let t = Template::parse("{TRACK}. {ARTIST} ({YEAR})");
        assert_eq!(t.render(&meta_full()), "3. Alice (2024)");
    }
}
