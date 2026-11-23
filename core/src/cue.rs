//! CUE sheet parsing for single-file albums (for example FLAC + CUE).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CueTrack {
    pub number: u32,
    pub title: Option<String>,
    pub performer: Option<String>,
    pub file: Option<String>,
    pub start_ms: u64,
    /// Start of the next track in the same file; `None` for the last one (use file length).
    pub end_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CueSheet {
    pub title: Option<String>,
    pub performer: Option<String>,
    pub tracks: Vec<CueTrack>,
}

struct Pending {
    number: u32,
    title: Option<String>,
    performer: Option<String>,
    file: Option<String>,
    start_ms: Option<u64>,
}

/// Parse a CUE sheet. Uses `INDEX 01` as the track start; unknown commands are ignored.
pub fn parse_cue(input: &str) -> CueSheet {
    let mut sheet = CueSheet::default();
    let mut file: Option<String> = None;
    let mut current: Option<Pending> = None;

    let finish = |pending: Option<Pending>, tracks: &mut Vec<CueTrack>| {
        if let Some(p) = pending {
            if let Some(start_ms) = p.start_ms {
                tracks.push(CueTrack {
                    number: p.number,
                    title: p.title,
                    performer: p.performer,
                    file: p.file,
                    start_ms,
                    end_ms: None,
                });
            }
        }
    };

    for raw in input.lines() {
        let line = raw.trim().trim_start_matches('\u{feff}');
        let (command, rest) = match line.split_once(char::is_whitespace) {
            Some((c, r)) => (c.to_ascii_uppercase(), r.trim()),
            None => (line.to_ascii_uppercase(), ""),
        };
        match command.as_str() {
            "FILE" => file = Some(first_field(rest)),
            "TRACK" => {
                finish(current.take(), &mut sheet.tracks);
                let number = rest
                    .split_whitespace()
                    .next()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(sheet.tracks.len() as u32 + 1);
                current = Some(Pending {
                    number,
                    title: None,
                    performer: None,
                    file: file.clone(),
                    start_ms: None,
                });
            }
            "TITLE" => match current.as_mut() {
                Some(track) => track.title = Some(unquote(rest)),
                None => sheet.title = Some(unquote(rest)),
            },
            "PERFORMER" => match current.as_mut() {
                Some(track) => track.performer = Some(unquote(rest)),
                None => sheet.performer = Some(unquote(rest)),
            },
            "INDEX" => {
                let mut parts = rest.split_whitespace();
                if let (Some(track), Some("01" | "1"), Some(time)) =
                    (current.as_mut(), parts.next(), parts.next())
                {
                    track.start_ms = parse_time(time);
                }
            }
            _ => {}
        }
    }
    finish(current.take(), &mut sheet.tracks);

    for i in 0..sheet.tracks.len() {
        let next_start = sheet
            .tracks
            .get(i + 1)
            .filter(|next| next.file == sheet.tracks[i].file)
            .map(|next| next.start_ms)
            .filter(|end| *end > sheet.tracks[i].start_ms);
        sheet.tracks[i].end_ms = next_start;
    }
    sheet
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    match value.strip_prefix('"') {
        Some(rest) => rest.split('"').next().unwrap_or("").to_owned(),
        None => value.to_owned(),
    }
}

/// First field of a command: a quoted string or the first whitespace-separated word.
fn first_field(value: &str) -> String {
    let value = value.trim();
    if value.starts_with('"') {
        unquote(value)
    } else {
        value.split_whitespace().next().unwrap_or("").to_owned()
    }
}

/// `mm:ss:ff` where `ff` is frames at 75 per second.
fn parse_time(value: &str) -> Option<u64> {
    let mut parts = value.split(':');
    let minutes: u64 = parts.next()?.parse().ok()?;
    let seconds: u64 = parts.next()?.parse().ok()?;
    let frames: u64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || seconds >= 60 || frames >= 75 {
        return None;
    }
    Some((minutes * 60 + seconds) * 1_000 + frames * 1_000 / 75)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHEET: &str = "REM GENRE Rock\nPERFORMER \"The Band\"\nTITLE \"Greatest Hits\"\nFILE \"album.flac\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"One\"\n    INDEX 00 00:00:00\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    TITLE \"Two\"\n    PERFORMER \"Guest\"\n    INDEX 01 03:30:37\n  TRACK 03 AUDIO\n    TITLE \"Three\"\n    INDEX 01 07:00:15\n";

    #[test]
    fn parses_album_and_track_boundaries() {
        let sheet = parse_cue(SHEET);
        assert_eq!(sheet.title.as_deref(), Some("Greatest Hits"));
        assert_eq!(sheet.performer.as_deref(), Some("The Band"));
        assert_eq!(sheet.tracks.len(), 3);
        assert_eq!(sheet.tracks[0].start_ms, 0);
        assert_eq!(sheet.tracks[1].start_ms, 210_000 + 37 * 1_000 / 75);
        assert_eq!(sheet.tracks[1].performer.as_deref(), Some("Guest"));
        assert_eq!(sheet.tracks[0].end_ms, Some(sheet.tracks[1].start_ms));
        assert_eq!(sheet.tracks[2].end_ms, None);
        assert_eq!(sheet.tracks[0].file.as_deref(), Some("album.flac"));
    }

    #[test]
    fn tracks_without_index_01_are_skipped_and_files_split_ranges() {
        let sheet = parse_cue("FILE a.wav WAVE\nTRACK 01 AUDIO\nINDEX 01 00:00:00\nFILE b.wav WAVE\nTRACK 02 AUDIO\nINDEX 01 00:00:00\nTRACK 03 AUDIO\nTITLE \"no index\"\n");
        assert_eq!(sheet.tracks.len(), 2);
        assert_eq!(sheet.tracks[0].end_ms, None);
        assert_eq!(sheet.tracks[1].file.as_deref(), Some("b.wav"));
    }

    #[test]
    fn rejects_invalid_times_and_handles_empty_input() {
        let sheet = parse_cue("TRACK 01 AUDIO\nINDEX 01 00:99:00\n");
        assert!(sheet.tracks.is_empty());
        assert_eq!(parse_cue(""), CueSheet::default());
    }
}
