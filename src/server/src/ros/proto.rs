//! RouterOS binary API framing: length-prefixed words, sentences end with an empty word.
//! Pure functions, no IO, so every edge case is unit-tested.

use std::collections::BTreeMap;

/// Longest word we accept from the router. RouterOS can send long values (log lines,
/// exports), but nothing we read is near this; it bounds memory if the peer misbehaves.
pub const MAX_WORD: usize = 1 << 20;
/// Most words in one sentence (one row has at most a few dozen attributes).
pub const MAX_WORDS: usize = 1024;

pub fn encode_len(len: usize, out: &mut Vec<u8>) {
    let n = len as u32;
    match len {
        0..=0x7f => out.push(n as u8),
        0x80..=0x3fff => out.extend_from_slice(&((n | 0x8000) as u16).to_be_bytes()),
        0x4000..=0x1f_ffff => out.extend_from_slice(&(n | 0xc0_0000).to_be_bytes()[1..]),
        0x20_0000..=0x0fff_ffff => out.extend_from_slice(&(n | 0xe000_0000).to_be_bytes()),
        _ => {
            out.push(0xf0);
            out.extend_from_slice(&n.to_be_bytes());
        }
    }
}

/// Decodes a word length from the start of `buf`. Returns (length, bytes used), or None if more bytes are needed.
pub fn decode_len(buf: &[u8]) -> Result<Option<(usize, usize)>, &'static str> {
    let Some(&b) = buf.first() else { return Ok(None) };
    let (extra, first) = match b {
        0x00..=0x7f => (0, b as u32),
        0x80..=0xbf => (1, (b & 0x3f) as u32),
        0xc0..=0xdf => (2, (b & 0x1f) as u32),
        0xe0..=0xef => (3, (b & 0x0f) as u32),
        0xf0 => (4, 0),
        _ => return Err("reserved control byte"),
    };
    if buf.len() < 1 + extra {
        return Ok(None);
    }
    let len = buf[1..=extra].iter().fold(first, |a, &x| (a << 8) | x as u32) as usize;
    if len > MAX_WORD {
        return Err("word too long");
    }
    Ok(Some((len, 1 + extra)))
}

pub fn encode_sentence(words: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    for w in words {
        encode_len(w.len(), &mut out);
        out.extend_from_slice(w.as_bytes());
    }
    out.push(0);
    out
}

/// Tries to take one complete sentence off the front of `buf`.
/// Returns the words and how many bytes they used, or None if the sentence is incomplete.
pub fn decode_sentence(buf: &[u8]) -> Result<Option<(Vec<String>, usize)>, &'static str> {
    let mut pos = 0;
    let mut words = Vec::new();
    loop {
        let Some((len, used)) = decode_len(&buf[pos..])? else { return Ok(None) };
        pos += used;
        if len == 0 {
            return Ok(Some((words, pos)));
        }
        if buf.len() < pos + len {
            return Ok(None);
        }
        // RouterOS sends UTF-8 for new data but older objects can hold Latin-1 bytes; never fail on them.
        words.push(String::from_utf8_lossy(&buf[pos..pos + len]).into_owned());
        pos += len;
        if words.len() > MAX_WORDS {
            return Err("sentence too long");
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Re,
    Done,
    Trap,
    Fatal,
    Empty,
}

/// One parsed reply sentence: `!re`/`!done`/`!trap`/`!fatal` plus its `=key=value` attributes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub kind: Kind,
    pub tag: Option<String>,
    pub attrs: BTreeMap<String, String>,
}

pub fn parse_reply(words: &[String]) -> Result<Reply, &'static str> {
    let kind = match words.first().map(String::as_str) {
        Some("!re") => Kind::Re,
        Some("!done") => Kind::Done,
        Some("!trap") => Kind::Trap,
        Some("!fatal") => Kind::Fatal,
        Some("!empty") => Kind::Empty,
        _ => return Err("unknown reply type"),
    };
    let mut tag = None;
    let mut attrs = BTreeMap::new();
    for w in &words[1..] {
        if let Some(t) = w.strip_prefix(".tag=") {
            tag = Some(t.to_string());
        } else if let Some(kv) = w.strip_prefix('=') {
            // the value may itself contain '=', so split on the first one only
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            attrs.insert(k.to_string(), v.to_string());
        } else if kind == Kind::Fatal {
            attrs.insert("message".into(), w.clone());
        }
    }
    Ok(Reply { kind, tag, attrs })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_round_trips_at_every_boundary() {
        for len in [0, 1, 0x7f, 0x80, 0x3fff, 0x4000, MAX_WORD] {
            let mut b = Vec::new();
            encode_len(len, &mut b);
            assert_eq!(decode_len(&b).unwrap(), Some((len, b.len())), "len {len:#x}");
        }
    }

    #[test]
    fn length_encodings_match_the_manual() {
        let enc = |n| {
            let mut b = Vec::new();
            encode_len(n, &mut b);
            b
        };
        assert_eq!(enc(0x7f), [0x7f]);
        assert_eq!(enc(0x80), [0x80, 0x80]);
        assert_eq!(enc(0x4000), [0xc0, 0x40, 0x00]);
        assert_eq!(enc(0x20_0000), [0xe0, 0x20, 0x00, 0x00]);
        assert_eq!(enc(0x1000_0000), [0xf0, 0x10, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn rejects_oversized_and_reserved() {
        assert!(decode_len(&[0xf0, 0x7f, 0, 0, 0]).is_err());
        assert!(decode_len(&[0xf8]).is_err());
    }

    #[test]
    fn partial_input_waits_for_more() {
        let s = encode_sentence(&["/login".into(), "=name=u".into()]);
        for cut in 0..s.len() {
            assert_eq!(decode_sentence(&s[..cut]).unwrap(), None, "cut {cut}");
        }
        let (w, used) = decode_sentence(&s).unwrap().unwrap();
        assert_eq!(used, s.len());
        assert_eq!(w, ["/login", "=name=u"]);
    }

    #[test]
    fn parses_reply_values_containing_equals() {
        let r = parse_reply(&["!re".into(), "=comment=a=b".into(), ".tag=7".into(), "=x".into()]).unwrap();
        assert_eq!(r.kind, Kind::Re);
        assert_eq!(r.tag.as_deref(), Some("7"));
        assert_eq!(r.attrs["comment"], "a=b");
        assert_eq!(r.attrs["x"], "");
    }

    #[test]
    fn fatal_keeps_its_message_and_unknown_is_an_error() {
        let r = parse_reply(&["!fatal".into(), "session terminated".into()]).unwrap();
        assert_eq!(r.attrs["message"], "session terminated");
        assert!(parse_reply(&["!what".into()]).is_err());
        assert!(parse_reply(&[]).is_err());
    }

    #[test]
    fn too_many_words_is_an_error() {
        let words: Vec<String> = (0..=MAX_WORDS).map(|_| "a".into()).collect();
        assert!(decode_sentence(&encode_sentence(&words)).is_err());
    }
}
