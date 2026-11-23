//! Synchronized lyrics (LRC) parsing and position lookup.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LyricLine {
    pub time_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Lyrics {
    lines: Vec<LyricLine>,
}

impl Lyrics {
    /// Parse LRC text. Supports several timestamps per line (`[00:10.00][00:50.00]text`),
    /// two- or three-digit fractions, and the `[offset:+/-ms]` tag. Metadata tags and lines
    /// without timestamps are ignored. Lines are returned sorted by time.
    pub fn parse_lrc(input: &str) -> Self {
        let mut offset_ms: i64 = 0;
        let mut lines = Vec::new();

        for raw in input.lines() {
            let mut rest = raw.trim().trim_start_matches('\u{feff}');
            let mut times = Vec::new();
            while let Some(tail) = rest.strip_prefix('[') {
                let Some(end) = tail.find(']') else { break };
                let tag = &tail[..end];
                if let Some(time) = parse_timestamp(tag) {
                    times.push(time);
                } else if let Some(value) = tag
                    .strip_prefix("offset:")
                    .and_then(|v| v.trim().parse::<i64>().ok())
                {
                    offset_ms = value;
                }
                rest = &tail[end + 1..];
            }
            if times.is_empty() {
                continue;
            }
            let text = rest.trim().to_owned();
            for time_ms in times {
                lines.push(LyricLine {
                    time_ms,
                    text: text.clone(),
                });
            }
        }

        // A positive offset makes lyrics appear earlier.
        for line in &mut lines {
            line.time_ms = (line.time_ms as i64 - offset_ms).max(0) as u64;
        }
        lines.sort_by_key(|line| line.time_ms);
        Self { lines }
    }

    pub fn lines(&self) -> &[LyricLine] {
        &self.lines
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Index of the line that is active at `position_ms`, or `None` before the first line.
    pub fn index_at(&self, position_ms: u64) -> Option<usize> {
        self.lines
            .partition_point(|line| line.time_ms <= position_ms)
            .checked_sub(1)
    }

    pub fn line_at(&self, position_ms: u64) -> Option<&LyricLine> {
        self.index_at(position_ms).map(|index| &self.lines[index])
    }
}

fn parse_timestamp(tag: &str) -> Option<u64> {
    let (minutes, seconds) = tag.split_once(':')?;
    if minutes.is_empty() || !minutes.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (whole, fraction) = match seconds.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => (seconds, ""),
    };
    if whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let mut millis: String = fraction.chars().take(3).collect();
    while millis.len() < 3 {
        millis.push('0');
    }
    let minutes: u64 = minutes.parse().ok()?;
    let whole: u64 = whole.parse().ok()?;
    Some(minutes * 60_000 + whole * 1_000 + millis.parse::<u64>().ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str =
        "[ar:Artist]\n[ti:Song]\n[00:12.00]First\n[00:05.50]Zero\n[01:02.345]Last\nplain text\n";

    #[test]
    fn parses_sorts_and_ignores_metadata() {
        let lyrics = Lyrics::parse_lrc(SAMPLE);
        let times: Vec<_> = lyrics.lines().iter().map(|l| l.time_ms).collect();
        assert_eq!(times, [5_500, 12_000, 62_345]);
        assert_eq!(lyrics.lines()[0].text, "Zero");
    }

    #[test]
    fn supports_multiple_timestamps_and_persian_text() {
        let lyrics = Lyrics::parse_lrc("[00:01.00][00:03.00]سلام\n");
        assert_eq!(lyrics.lines().len(), 2);
        assert_eq!(lyrics.lines()[1].text, "سلام");
    }

    #[test]
    fn finds_active_line() {
        let lyrics = Lyrics::parse_lrc(SAMPLE);
        assert!(lyrics.line_at(1_000).is_none());
        assert_eq!(lyrics.line_at(5_500).unwrap().text, "Zero");
        assert_eq!(lyrics.line_at(30_000).unwrap().text, "First");
        assert_eq!(lyrics.line_at(999_999).unwrap().text, "Last");
    }

    #[test]
    fn offset_tag_shifts_lines_and_never_underflows() {
        let lyrics = Lyrics::parse_lrc("[offset:+1000]\n[00:00.50]a\n[00:05.00]b\n");
        let times: Vec<_> = lyrics.lines().iter().map(|l| l.time_ms).collect();
        assert_eq!(times, [0, 4_000]);
    }

    #[test]
    fn garbage_input_yields_empty_lyrics() {
        assert!(Lyrics::parse_lrc("[x:y]\n[00:zz]\n[").is_empty());
        assert!(Lyrics::parse_lrc("").is_empty());
    }
}
