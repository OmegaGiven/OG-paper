//! Spell check, never autocorrect: while typing, words not in the
//! dictionary are underlined, and the one at the cursor offers spellings to
//! pick, Add to dictionary and Ignore. The native apps check with a bundled
//! English (US) Hunspell dictionary (dict/, SCOWL: MIT and BSD); on the
//! web the browser's own spell check does it (see og-web.js).

#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashMap;
use std::collections::HashSet;
use std::ops::Range;
use std::sync::Mutex;

/// Words the person added (lowercase), kept in the prefs as `spellwords`.
static ADDED: Mutex<Option<HashSet<String>>> = Mutex::new(None);
/// Words ignored until the app closes (lowercase).
static IGNORED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

#[cfg(not(target_arch = "wasm32"))]
mod dict {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::OnceLock;

    static DICT: OnceLock<spellbook::Dictionary> = OnceLock::new();
    static LOADING: AtomicBool = AtomicBool::new(false);

    /// Read the dictionary in the background (it takes a moment), once.
    pub fn warm() {
        if LOADING.swap(true, Ordering::SeqCst) {
            return;
        }
        let load = || {
            let aff = include_str!("../dict/en_US.aff");
            let dic = include_str!("../dict/en_US.dic");
            if let Ok(d) = spellbook::Dictionary::new(aff, dic) {
                let _ = DICT.set(d);
            }
        };
        if std::thread::Builder::new().name("spell".into()).spawn(load).is_err() {
            load();
        }
    }

    pub fn get() -> Option<&'static spellbook::Dictionary> {
        DICT.get()
    }

    /// Wait for the dictionary (tests).
    #[cfg(test)]
    pub(crate) fn wait() {
        warm();
        while DICT.get().is_none() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

/// Start reading the dictionary, so it is ready by the first text.
pub fn warm() {
    #[cfg(not(target_arch = "wasm32"))]
    dict::warm();
}

/// The words of `text` worth checking, as byte ranges: runs of letters
/// (with apostrophes inside), leaving out links, addresses, words with
/// digits, single letters and all-capital abbreviations.
pub fn words(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut chunk_start = 0;
    // Whitespace-separated chunks first: a link or address is skipped whole.
    for (i, c) in text.char_indices().chain(std::iter::once((text.len(), ' '))) {
        if !c.is_whitespace() {
            continue;
        }
        let chunk = &text[chunk_start..i];
        let link = chunk.contains("://")
            || chunk.contains('@')
            || chunk.starts_with("www.")
            || chunk.contains(['/', '\\', '_', '#']);
        if !link {
            words_in(chunk, chunk_start, &mut out);
        }
        chunk_start = i + c.len_utf8();
    }
    out
}

fn words_in(chunk: &str, base: usize, out: &mut Vec<Range<usize>>) {
    let is_part = |c: char| c.is_alphanumeric() || c == '\'' || c == '’';
    let mut start: Option<usize> = None;
    for (i, c) in chunk.char_indices().chain(std::iter::once((chunk.len(), ' '))) {
        match (start, is_part(c)) {
            (None, true) => start = Some(i),
            (Some(s), false) => {
                // Apostrophes at the ends are quotes, not the word's.
                let w = &chunk[s..i];
                let lead = w.len() - w.trim_start_matches(['\'', '’']).len();
                let w = w.trim_matches(['\'', '’']);
                let s = s + lead;
                let letters = w.chars().filter(|c| c.is_alphabetic()).count();
                let digits = w.chars().any(|c| c.is_numeric());
                let caps = w.chars().all(|c| !c.is_lowercase());
                if letters >= 2 && !digits && !caps {
                    out.push(base + s..base + s + w.len());
                }
                start = None;
            }
            _ => {}
        }
    }
}

fn key(word: &str) -> String {
    word.replace('’', "'").to_lowercase()
}

/// Whether `word` is spelled right (or added, or ignored). True until the
/// dictionary is ready, and always on the web.
pub fn check(word: &str) -> bool {
    let k = key(word);
    let listed = |m: &Mutex<Option<HashSet<String>>>| {
        m.lock()
            .ok()
            .is_some_and(|s| s.as_ref().is_some_and(|s| s.contains(&k)))
    };
    if listed(&ADDED) || listed(&IGNORED) {
        return true;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        static SEEN: Mutex<Option<HashMap<String, bool>>> = Mutex::new(None);
        let Some(d) = dict::get() else {
            return true;
        };
        let w = word.replace('’', "'");
        if let Ok(mut seen) = SEEN.lock() {
            let seen = seen.get_or_insert_with(HashMap::new);
            if let Some(&ok) = seen.get(&w) {
                return ok;
            }
            let ok = d.check(&w);
            if seen.len() > 20_000 {
                seen.clear();
            }
            seen.insert(w, ok);
            return ok;
        }
        d.check(&w)
    }
    #[cfg(target_arch = "wasm32")]
    true
}

/// The misspelled words of `text` (byte ranges).
pub fn misspelled(text: &str) -> Vec<Range<usize>> {
    words(text)
        .into_iter()
        .filter(|r| !check(&text[r.clone()]))
        .collect()
}

/// Up to five spellings for `word`, best first.
pub fn suggest(word: &str) -> Vec<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        static MEMO: Mutex<Option<(String, Vec<String>)>> = Mutex::new(None);
        let Some(d) = dict::get() else {
            return Vec::new();
        };
        if let Ok(m) = MEMO.lock() {
            if let Some((w, s)) = m.as_ref().filter(|(w, _)| w == word) {
                let _ = w;
                return s.clone();
            }
        }
        let mut out = Vec::new();
        d.suggest(&word.replace('’', "'"), &mut out);
        out.dedup();
        out.truncate(5);
        if let Ok(mut m) = MEMO.lock() {
            *m = Some((word.to_string(), out.clone()));
        }
        out
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = word;
        Vec::new()
    }
}

/// The added words, from the prefs (at start, and when another set of
/// settings is put in place).
pub fn set_added(list: &str) {
    if let Ok(mut a) = ADDED.lock() {
        *a = Some(list.split_whitespace().map(key).collect());
    }
}

/// Add `word` to the person's dictionary (kept in the prefs).
pub fn add(word: &str) {
    let mut p = crate::prefs::load();
    let mut list: Vec<String> = p
        .get("spellwords")
        .map(|s| s.split_whitespace().map(String::from).collect())
        .unwrap_or_default();
    let k = key(word);
    if !list.contains(&k) {
        list.push(k);
    }
    let text = list.join(" ");
    set_added(&text);
    p.insert("spellwords".into(), text);
    crate::prefs::save(&p);
}

/// Pass over `word` until the app closes.
pub fn ignore(word: &str) {
    if let Ok(mut s) = IGNORED.lock() {
        s.get_or_insert_with(HashSet::new).insert(key(word));
    }
}

/// The word (byte range) of `text` at or just before char index `cursor`.
pub fn word_at(text: &str, cursor: usize) -> Option<Range<usize>> {
    let at = text
        .char_indices()
        .nth(cursor)
        .map_or(text.len(), |(i, _)| i);
    words(text)
        .into_iter()
        .find(|r| r.start <= at && at <= r.end)
}

/// Wait for the dictionary (tests).
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn wait() {
    dict::wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_leave_out_links_numbers_and_abbreviations() {
        let t = "Hello 'wrold', see https://x.io/abc me@x.com NASA a 3rd don't";
        let w: Vec<&str> = words(t).into_iter().map(|r| &t[r]).collect();
        assert_eq!(w, ["Hello", "wrold", "see", "don't"]);
    }

    #[test]
    fn misspellings_are_found_and_spellings_offered_never_changed() {
        dict::wait();
        let t = "The quick brwon fox doesn't jumpp";
        let bad: Vec<&str> = misspelled(t).into_iter().map(|r| &t[r]).collect();
        assert_eq!(bad, ["brwon", "jumpp"]);
        assert!(suggest("brwon").iter().any(|s| s == "brown"), "{:?}", suggest("brwon"));
        // Ignored, then fine.
        ignore("Jumpp");
        assert!(check("jumpp"));
        let r = word_at(t, 12).expect("a word");
        assert_eq!(&t[r], "brwon");
    }
}
