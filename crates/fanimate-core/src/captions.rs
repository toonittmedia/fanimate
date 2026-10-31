//! Captions: reading and writing SubRip (`.srt`) and WebVTT (`.vtt`).
//!
//! Only timing and words are kept; positioning settings, styles and notes are
//! left out, and simple formatting tags are stripped, because a caption in
//! Fanimate is styled by its layer. The parser is for files of unknown
//! origin: it never panics, caps what it reads, and skips a cue it cannot
//! make sense of, counting it, rather than refusing the whole file.

use std::fmt::Write as _;

/// The longest file read, in bytes.
pub const MAX_BYTES: usize = 20 * 1024 * 1024;
/// The most cues read.
pub const MAX_CUES: usize = 50_000;
/// The most characters kept of one cue's text.
pub const MAX_CUE_CHARS: usize = 2_000;
/// Cues may not start later than this (100 hours): further is a typo.
pub const MAX_MS: u64 = 100 * 60 * 60 * 1000;

/// One caption: shown from `start_ms` until `end_ms`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cue {
    pub start_ms: u64,
    pub end_ms: u64,
    /// Lines are joined with `\n`.
    pub text: String,
}

/// What was read: the cues in start order, and how many were left out.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Parsed {
    pub cues: Vec<Cue>,
    /// Cues skipped for a bad time, an end before the start, or no words.
    pub skipped: usize,
    /// Cues beyond [`MAX_CUES`] that were not read.
    pub cut_off: usize,
}

/// Why a file was not read at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptionError {
    TooLarge,
    NoCues,
}

impl std::fmt::Display for CaptionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => write!(
                f,
                "the caption file is larger than {} MB",
                MAX_BYTES / 1024 / 1024
            ),
            Self::NoCues => write!(f, "no captions were found: this is not an SRT or WebVTT file"),
        }
    }
}

impl std::error::Error for CaptionError {}

/// Read SubRip or WebVTT (they are told apart by what is in them, not by
/// the file name).
pub fn parse(source: &str) -> Result<Parsed, CaptionError> {
    if source.len() > MAX_BYTES {
        return Err(CaptionError::TooLarge);
    }
    let text = source
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let mut parsed = Parsed::default();
    for block in text.split("\n\n").flat_map(split_on_blank_lines) {
        let lines: Vec<&str> = block.lines().collect();
        let Some(at) = lines.iter().position(|l| l.contains("-->")) else {
            continue;
        };
        // a block whose first line is a header, note or style is not a cue
        if lines.first().is_some_and(|l| is_metadata(l)) {
            continue;
        }
        if at > 1 {
            continue;
        }
        let Some((start, end)) = timing(lines[at]) else {
            parsed.skipped += 1;
            continue;
        };
        let words = clean(&lines[at + 1..]);
        if end < start || words.is_empty() {
            parsed.skipped += 1;
            continue;
        }
        if parsed.cues.len() >= MAX_CUES {
            parsed.cut_off += 1;
            continue;
        }
        parsed.cues.push(Cue {
            start_ms: start,
            end_ms: end,
            text: words,
        });
    }
    if parsed.cues.is_empty() && parsed.skipped == 0 {
        return Err(CaptionError::NoCues);
    }
    parsed.cues.sort_by_key(|c| c.start_ms);
    Ok(parsed)
}

/// A block can hold two cues when the blank line between them has stray
/// spaces in it; split those too.
fn split_on_blank_lines(block: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    for line in block.lines() {
        if line.trim().is_empty() {
            if !out.last().is_some_and(String::is_empty) {
                out.push(String::new());
            }
        } else if let Some(current) = out.last_mut() {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
    }
    out.retain(|b| !b.is_empty());
    out
}

fn is_metadata(line: &str) -> bool {
    let line = line.trim();
    line.starts_with("WEBVTT") || line.starts_with("NOTE") || line == "STYLE" || line == "REGION"
}

/// `00:00:01,000 --> 00:00:02,500 position:10%` as two times in milliseconds.
fn timing(line: &str) -> Option<(u64, u64)> {
    let (a, rest) = line.split_once("-->")?;
    let b = rest.split_whitespace().next()?;
    Some((time(a.trim())?, time(b)?))
}

/// `hh:mm:ss,mmm`, `hh:mm:ss.mmm` or `mm:ss.mmm`.
fn time(s: &str) -> Option<u64> {
    let (clock, fraction) = match s.split_once([',', '.']) {
        Some((c, f)) => (c, f),
        None => (s, "0"),
    };
    if fraction.is_empty() || fraction.len() > 9 || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // "5" after the point is half a second, not five milliseconds
    let millis: u64 = format!("{fraction:0<3}")[..3].parse().ok()?;
    let parts: Vec<&str> = clock.split(':').collect();
    let numbers: Option<Vec<u64>> = parts
        .iter()
        .map(|p| {
            (!p.is_empty() && p.len() <= 6 && p.bytes().all(|b| b.is_ascii_digit()))
                .then(|| p.parse().ok())
                .flatten()
        })
        .collect();
    let (h, m, sec) = match numbers?.as_slice() {
        [h, m, s] => (*h, *m, *s),
        [m, s] => (0, *m, *s),
        _ => return None,
    };
    if m > 59 || sec > 59 {
        return None;
    }
    let ms = ((h * 60 + m) * 60 + sec) * 1000 + millis;
    (ms <= MAX_MS).then_some(ms)
}

/// The words of a cue: tags and `{\an8}` positions removed, entities
/// decoded, lines trimmed, the whole kept to [`MAX_CUE_CHARS`].
fn clean(lines: &[&str]) -> String {
    let mut out = String::new();
    for line in lines {
        let mut text = String::new();
        let mut in_tag = false;
        let mut in_brace = false;
        for c in line.chars() {
            match c {
                '<' => in_tag = true,
                '>' if in_tag => in_tag = false,
                '{' if !in_tag => in_brace = true,
                '}' if in_brace => in_brace = false,
                _ if in_tag || in_brace => {}
                _ => text.push(c),
            }
        }
        let text = text
            .replace("&nbsp;", " ")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&");
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(text);
    }
    out.chars().take(MAX_CUE_CHARS).collect()
}

fn clock(ms: u64, separator: char) -> String {
    let (h, m, s, f) = (ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000);
    format!("{h:02}:{m:02}:{s:02}{separator}{f:03}")
}

/// SubRip: numbered cues with comma times.
pub fn to_srt(cues: &[Cue]) -> String {
    let mut out = String::new();
    for (n, cue) in cues.iter().enumerate() {
        let _ = write!(
            out,
            "{}\n{} --> {}\n{}\n\n",
            n + 1,
            clock(cue.start_ms, ','),
            clock(cue.end_ms, ','),
            cue.text
        );
    }
    out
}

/// WebVTT: a header, then cues with point times. A line in a cue that is
/// empty, or that holds `-->`, would end or confuse it and is changed.
pub fn to_vtt(cues: &[Cue]) -> String {
    let mut out = String::from("WEBVTT\n\n");
    for cue in cues {
        let text = cue
            .text
            .replace("-->", "->")
            .replace('&', "&amp;")
            .replace('<', "&lt;");
        let _ = write!(
            out,
            "{} --> {}\n{}\n\n",
            clock(cue.start_ms, '.'),
            clock(cue.end_ms, '.'),
            text
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRT: &str = "1\r\n00:00:01,000 --> 00:00:03,500\r\nHello there.\r\n\r\n2\r\n00:00:04,000 --> 00:00:06,000\r\nTwo lines\r\nof words\r\n\r\n";

    #[test]
    fn subrip_is_read_with_its_numbers_times_and_lines() {
        let p = parse(SRT).unwrap();
        assert_eq!(p.skipped, 0);
        assert_eq!(
            p.cues,
            vec![
                Cue {
                    start_ms: 1000,
                    end_ms: 3500,
                    text: "Hello there.".into()
                },
                Cue {
                    start_ms: 4000,
                    end_ms: 6000,
                    text: "Two lines\nof words".into()
                },
            ]
        );
    }

    #[test]
    fn webvtt_is_read_without_its_header_notes_styles_identifiers_and_settings() {
        let vtt = "\u{feff}WEBVTT - a title\n\nNOTE this is a note\nstill the note\n\nSTYLE\n::cue { color: red }\n\nintro\n00:01.500 --> 00:03.000 position:10% align:start\n<v Ann><b>Hi</b> &amp; <i>welcome</i></v>\n\n01:00:00.250 --> 01:00:01.000\nAn hour in\n";
        let p = parse(vtt).unwrap();
        assert_eq!(p.cues.len(), 2, "{p:?}");
        assert_eq!(p.cues[0].start_ms, 1500);
        assert_eq!(p.cues[0].text, "Hi & welcome");
        assert_eq!(p.cues[1].start_ms, 3_600_250);
    }

    #[test]
    fn a_fraction_is_a_fraction_of_a_second() {
        assert_eq!(time("00:00:01.5"), Some(1500));
        assert_eq!(time("00:00:01,05"), Some(1050));
        assert_eq!(time("00:00:01"), Some(1000));
        assert_eq!(time("00:00:01.1234567"), Some(1123));
    }

    #[test]
    fn bad_times_are_not_times() {
        for bad in [
            "",
            "1",
            "00:61:00,000",
            "00:00:61,000",
            "a:b:c,d",
            "00:00:01,",
            "-1:00",
            "999999:00:00,000",
            "00:00:00:00,000",
        ] {
            assert_eq!(time(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn cues_that_make_no_sense_are_skipped_and_counted_not_fatal() {
        let src = "1\n00:00:05,000 --> 00:00:04,000\nBackwards\n\n2\nnonsense --> later\nBad time\n\n3\n00:00:07,000 --> 00:00:08,000\n   \n\n4\n00:00:09,000 --> 00:00:10,000\nGood\n";
        let p = parse(src).unwrap();
        assert_eq!(p.cues.len(), 1);
        assert_eq!(p.cues[0].text, "Good");
        assert_eq!(p.skipped, 3);
    }

    #[test]
    fn cues_come_out_in_start_order() {
        let src = "00:00:09,000 --> 00:00:10,000\nLate\n\n00:00:01,000 --> 00:00:02,000\nEarly\n";
        let p = parse(src).unwrap();
        assert_eq!(p.cues[0].text, "Early");
    }

    #[test]
    fn something_that_is_not_captions_is_an_error() {
        assert_eq!(parse("just some prose\n\nand more"), Err(CaptionError::NoCues));
        assert_eq!(parse(""), Err(CaptionError::NoCues));
        assert_eq!(parse("WEBVTT\n\n"), Err(CaptionError::NoCues));
        assert_eq!(parse(&"x".repeat(MAX_BYTES + 1)), Err(CaptionError::TooLarge));
    }

    #[test]
    fn the_number_of_cues_and_the_length_of_one_are_capped() {
        let mut src = String::new();
        for n in 0..(MAX_CUES + 5) {
            let s = n as u64 * 10;
            src.push_str(&format!(
                "{}\n{} --> {}\nw\n\n",
                n + 1,
                clock(s, ','),
                clock(s + 5, ',')
            ));
        }
        let p = parse(&src).unwrap();
        assert_eq!((p.cues.len(), p.cut_off), (MAX_CUES, 5));
        let long = format!(
            "00:00:01,000 --> 00:00:02,000\n{}\n",
            "y".repeat(MAX_CUE_CHARS * 3)
        );
        assert_eq!(parse(&long).unwrap().cues[0].text.chars().count(), MAX_CUE_CHARS);
    }

    #[test]
    fn what_is_written_reads_back_the_same_in_both_forms() {
        let cues = vec![
            Cue {
                start_ms: 0,
                end_ms: 999,
                text: "First".into(),
            },
            Cue {
                start_ms: 3_723_004,
                end_ms: 3_725_000,
                text: "Line one\nLine two & more <b>".into(),
            },
        ];
        let srt = parse(&to_srt(&cues)).unwrap();
        // tags are stripped when read, so what was written as "<b>" does not come back
        assert_eq!(srt.cues[0], cues[0]);
        assert_eq!(srt.cues[1].start_ms, cues[1].start_ms);
        let vtt = parse(&to_vtt(&cues)).unwrap();
        assert_eq!(vtt.cues[0], cues[0]);
        assert_eq!(
            vtt.cues[1].text, cues[1].text,
            "WebVTT escapes what would read as a tag"
        );
        assert!(to_vtt(&cues).starts_with("WEBVTT\n\n"));
        assert!(to_srt(&cues).starts_with("1\n00:00:00,000 --> 00:00:00,999\nFirst\n"));
    }

    #[test]
    fn a_caption_with_an_arrow_in_it_cannot_break_the_file_it_is_written_to() {
        let cues = vec![Cue {
            start_ms: 0,
            end_ms: 1000,
            text: "a --> b".into(),
        }];
        let again = parse(&to_vtt(&cues)).unwrap();
        assert_eq!(again.cues.len(), 1);
        assert_eq!(again.cues[0].text, "a -> b");
    }

    #[test]
    fn random_bytes_and_hostile_text_never_panic() {
        let mut seed = 12345u64;
        let pieces = [
            "-->",
            "\n",
            "\n\n",
            "00:00:01,000",
            "<",
            ">",
            "{",
            "}",
            "9",
            ":",
            ",",
            ".",
            "é",
            "\u{0}",
            "NOTE",
            "WEBVTT",
            " ",
        ];
        for _ in 0..2000 {
            let mut s = String::new();
            for _ in 0..40 {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                s.push_str(pieces[(seed >> 33) as usize % pieces.len()]);
            }
            let _ = parse(&s);
        }
    }
}
