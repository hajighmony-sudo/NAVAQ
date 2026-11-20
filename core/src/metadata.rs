//! Best-effort metadata guessing from file names, for files without usable tags.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuessedMetadata {
    pub track_number: Option<u32>,
    pub artist: Option<String>,
    pub title: String,
}

/// Guess track number, artist and title from a path such as
/// `/Music/03 - Artist - Title.mp3` or `Artist_-_Title.flac`.
pub fn guess_from_filename(path: &str) -> GuessedMetadata {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let stem = match name.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty()
                && !ext.is_empty()
                && ext.len() <= 5
                && ext.bytes().all(|b| b.is_ascii_alphanumeric()) =>
        {
            stem
        }
        _ => name,
    };
    let mut text = if stem.contains(' ') {
        stem.to_owned()
    } else {
        stem.replace('_', " ")
    };
    text = text.trim().to_owned();

    let mut track_number = None;
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    if (1..=3).contains(&digits) && digits < text.len() {
        let separator = text.as_bytes()[digits];
        let accepted = match separator {
            b'.' | b'-' | b')' => true,
            b' ' => digits >= 2,
            _ => false,
        };
        let rest = text[digits..].trim_start_matches(['.', '-', ')', ' ']);
        if accepted && !rest.is_empty() {
            track_number = text[..digits].parse().ok();
            text = rest.to_owned();
        }
    }

    match text.split_once(" - ") {
        Some((artist, title)) if !artist.trim().is_empty() && !title.trim().is_empty() => {
            GuessedMetadata {
                track_number,
                artist: Some(artist.trim().to_owned()),
                title: title.trim().to_owned(),
            }
        }
        _ => GuessedMetadata {
            track_number,
            artist: None,
            title: text,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guess(path: &str) -> (Option<u32>, Option<&'static str>, String) {
        let g = guess_from_filename(path);
        (
            g.track_number,
            g.artist
                .map(|a| Box::leak(a.into_boxed_str()) as &'static str),
            g.title,
        )
    }

    #[test]
    fn number_artist_title() {
        assert_eq!(
            guess("/Music/03 - Daft Punk - One More Time.mp3"),
            (Some(3), Some("Daft Punk"), "One More Time".into())
        );
    }

    #[test]
    fn underscores_and_windows_paths() {
        assert_eq!(
            guess("C:\\m\\Artist_-_Title.flac"),
            (None, Some("Artist"), "Title".into())
        );
    }

    #[test]
    fn plain_title_and_non_track_numbers() {
        assert_eq!(guess("Song.mp3"), (None, None, "Song".into()));
        assert_eq!(guess("1999.mp3"), (None, None, "1999".into()));
        assert_eq!(guess("7 Rings.mp3"), (None, None, "7 Rings".into()));
        assert_eq!(guess("05. Title.m4a"), (Some(5), None, "Title".into()));
    }

    #[test]
    fn persian_names_and_dots_in_titles() {
        assert_eq!(
            guess("01 - علی - سلام.mp3"),
            (Some(1), Some("علی"), "سلام".into())
        );
        assert_eq!(guess("Mr. Brightside").2, "Mr. Brightside");
    }
}
