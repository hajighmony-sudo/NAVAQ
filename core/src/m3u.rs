//! M3U / M3U8 playlist import and export (pure text, no file access).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct M3uEntry {
    pub location: String,
    pub title: Option<String>,
    pub duration_secs: Option<u64>,
}

/// Parse M3U text. `#EXTINF` metadata applies to the next location line; other comments,
/// blank lines and a UTF-8 BOM are ignored. An EXTINF duration of -1 means unknown.
pub fn parse(input: &str) -> Vec<M3uEntry> {
    let mut entries = Vec::new();
    let mut pending: Option<(Option<u64>, Option<String>)> = None;

    for line in input.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(info) = line.strip_prefix("#EXTINF:") {
            let (head, title) = match info.split_once(',') {
                Some((head, title)) => (head, title.trim()),
                None => (info, ""),
            };
            let duration = head
                .split_whitespace()
                .next()
                .and_then(|n| n.parse::<i64>().ok())
                .and_then(|n| u64::try_from(n).ok());
            let title = (!title.is_empty()).then(|| title.to_owned());
            pending = Some((duration, title));
        } else if line.starts_with('#') {
            continue;
        } else {
            let (duration_secs, title) = pending.take().unwrap_or((None, None));
            entries.push(M3uEntry {
                location: line.to_owned(),
                title,
                duration_secs,
            });
        }
    }
    entries
}

/// Render entries as extended M3U.
pub fn write(entries: &[M3uEntry]) -> String {
    let mut out = String::from("#EXTM3U\n");
    for entry in entries {
        if entry.title.is_some() || entry.duration_secs.is_some() {
            let duration = entry
                .duration_secs
                .map_or_else(|| "-1".to_owned(), |d| d.to_string());
            let title = entry
                .title
                .as_deref()
                .unwrap_or("")
                .replace(['\r', '\n'], " ");
            out.push_str(&format!("#EXTINF:{duration},{title}\n"));
        }
        out.push_str(&entry.location.replace(['\r', '\n'], ""));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_extinf_and_plain_entries() {
        let text = "\u{feff}#EXTM3U\n#EXTINF:215,Artist - Song\n/music/a.mp3\n\n# comment\nb.flac\n#EXTINF:-1,Live\nhttp://x/y\n";
        let entries = parse(text);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].duration_secs, Some(215));
        assert_eq!(entries[0].title.as_deref(), Some("Artist - Song"));
        assert_eq!(entries[1].title, None);
        assert_eq!(entries[2].duration_secs, None);
        assert_eq!(entries[2].title.as_deref(), Some("Live"));
    }

    #[test]
    fn write_then_parse_round_trips() {
        let entries = vec![
            M3uEntry {
                location: "/m/a.mp3".into(),
                title: Some("آهنگ".into()),
                duration_secs: Some(60),
            },
            M3uEntry {
                location: "b.mp3".into(),
                title: None,
                duration_secs: None,
            },
        ];
        assert_eq!(parse(&write(&entries)), entries);
    }

    #[test]
    fn extinf_with_attributes_and_garbage_is_tolerated() {
        let entries = parse("#EXTINF:12 tvg-id=\"x\",Name\nsong.mp3\n#EXTINF:oops\nother.mp3\n");
        assert_eq!(entries[0].duration_secs, Some(12));
        assert_eq!(entries[0].title.as_deref(), Some("Name"));
        assert_eq!(entries[1].duration_secs, None);
    }

    #[test]
    fn newlines_cannot_inject_extra_lines() {
        let out = write(&[M3uEntry {
            location: "a.mp3\nevil.mp3".into(),
            title: Some("x\ny".into()),
            duration_secs: None,
        }]);
        assert_eq!(parse(&out).len(), 1);
    }
}
