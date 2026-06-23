use std::borrow::Cow;

/// Clean source text immediately before it is sent to RWKV.
///
/// Keep this deliberately narrow. It is in the hot translation path, so the
/// function borrows the original text when no cleanup is needed and only
/// allocates after it finds repeated punctuation. Repeated punctuation is
/// collapsed even when whitespace appears between punctuation marks, e.g.
/// `. . . .` becomes `.`.
pub(crate) fn clean_text_for_rwkv(input: &str) -> Cow<'_, str> {
    if !has_repeated_punctuation(input) {
        return Cow::Borrowed(input);
    }

    Cow::Owned(collapse_repeated_punctuation(input))
}

fn has_repeated_punctuation(input: &str) -> bool {
    let mut pending_punctuation = None;
    let mut pending_whitespace = false;

    for ch in input.chars() {
        if Some(ch) == pending_punctuation && is_repeatable_punctuation(ch) {
            return true;
        }

        if ch.is_whitespace() {
            if pending_punctuation.is_some() {
                pending_whitespace = true;
            }
            continue;
        }

        if pending_whitespace && Some(ch) == pending_punctuation {
            return true;
        }

        pending_punctuation = is_repeatable_punctuation(ch).then_some(ch);
        pending_whitespace = false;
    }

    false
}

fn collapse_repeated_punctuation(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut last_punctuation = None;
    let mut pending_spaces = String::new();

    for ch in input.chars() {
        if ch.is_whitespace() {
            if last_punctuation.is_some() {
                pending_spaces.push(ch);
            } else {
                output.push(ch);
            }
            continue;
        }

        if Some(ch) == last_punctuation {
            pending_spaces.clear();
            continue;
        }

        output.push_str(&pending_spaces);
        pending_spaces.clear();
        output.push(ch);
        last_punctuation = is_repeatable_punctuation(ch).then_some(ch);
    }

    output.push_str(&pending_spaces);
    output
}

fn is_repeatable_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '.'
            | ','
            | '!'
            | '?'
            | ';'
            | ':'
            | '\u{3002}' // ideographic full stop
            | '\u{ff0c}' // fullwidth comma
            | '\u{3001}' // ideographic comma
            | '\u{ff01}' // fullwidth exclamation mark
            | '\u{ff1f}' // fullwidth question mark
            | '\u{ff1b}' // fullwidth semicolon
            | '\u{ff1a}' // fullwidth colon
    )
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::clean_text_for_rwkv;

    #[test]
    fn borrows_text_when_no_cleanup_is_needed() {
        let input = "Beautiful is better than ugly.";
        assert!(matches!(clean_text_for_rwkv(input), Cow::Borrowed(_)));
    }

    #[test]
    fn collapses_repeated_cjk_punctuation() {
        assert_eq!(
            clean_text_for_rwkv("dirty\u{3002}\u{3002}\u{3002}").as_ref(),
            "dirty\u{3002}"
        );
        assert_eq!(
            clean_text_for_rwkv("really\u{ff1f}\u{ff1f}\u{ff01}\u{ff01}").as_ref(),
            "really\u{ff1f}\u{ff01}"
        );
    }

    #[test]
    fn collapses_repeated_ascii_punctuation() {
        assert_eq!(clean_text_for_rwkv("Wait!!!!!!").as_ref(), "Wait!");
        assert_eq!(clean_text_for_rwkv("A,,,, B....").as_ref(), "A, B.");
    }

    #[test]
    fn collapses_repeated_ascii_punctuation_separated_by_spaces() {
        assert_eq!(
            clean_text_for_rwkv("Encrypted Traffic . . . . . . . . . . 3 1").as_ref(),
            "Encrypted Traffic . 3 1"
        );
        assert_eq!(clean_text_for_rwkv("A ? ? ? B").as_ref(), "A ? B");
    }

    #[test]
    fn keeps_spaces_between_different_punctuation_marks() {
        assert_eq!(clean_text_for_rwkv("What ? !").as_ref(), "What ? !");
    }

    #[test]
    fn does_not_collapse_repeated_letters_or_spaces() {
        assert_eq!(clean_text_for_rwkv("soooo  good").as_ref(), "soooo  good");
    }
}
