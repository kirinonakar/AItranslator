pub fn resolve_source_language<'a>(
    source_lang: &'a str,
    text: &str,
    document_source_lang: &str,
) -> &'a str {
    if source_lang != "Auto Detect" {
        return source_lang;
    }

    let mut kana = 0usize;
    let mut han = 0usize;
    let mut other_letters = 0usize;

    for character in text.chars() {
        match character as u32 {
            0x3041..=0x3096
            | 0x309d..=0x309f
            | 0x30a1..=0x30fa
            | 0x30fd..=0x30ff
            | 0x31f0..=0x31ff
            | 0xff66..=0xff6f
            | 0xff71..=0xff9d
            | 0x1b000..=0x1b122 => kana += 1,
            0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff | 0x20000..=0x323af => han += 1,
            // Voicing and vowel-length marks are not language evidence alone.
            0x3099..=0x309c | 0x30fc | 0xff70 | 0xff9e | 0xff9f => {}
            _ if character.is_alphabetic() => other_letters += 1,
            _ => {}
        }
    }

    // Kana distinguishes Japanese from Han-only text. Ignore punctuation and
    // avoid overriding another language just because it quotes a Japanese word.
    if kana > 0 && kana + han >= other_letters {
        return "Japanese";
    }

    // Preserve document context for kanji/number-only chunks, while letting the
    // model detect passages in other languages independently.
    if document_source_lang == "Japanese" && kana + han >= other_letters {
        return "Japanese";
    }
    "Auto Detect"
}

pub fn create_formatted_prompt(
    source_lang: &str,
    target_lang: &str,
    text: &str,
    document_source_lang: &str,
) -> String {
    let resolved_source = resolve_source_language(source_lang, text, document_source_lang);
    let source_code = language_code(resolved_source);
    let target_code = language_code(target_lang);
    let introduction = if resolved_source == "Auto Detect" {
        format!(
            "You are a professional translator. Identify the source language from the text itself and translate it into {target_lang} ({target_code}). Do not infer the source language from the requested target language."
        )
    } else {
        format!(
            "You are a professional {resolved_source} ({source_code}) to {target_lang} ({target_code}) translator."
        )
    };
    let japanese_instruction = if resolved_source == "Japanese" && target_lang != "Japanese" {
        format!(
            "\nThe source text is Japanese. Read kanji together with hiragana and katakana as Japanese, including short fragments. Translate the Japanese prose into {target_lang}; do not return the Japanese original as the translation."
        )
    } else {
        String::new()
    };

    format!(
        "{introduction}{japanese_instruction}\nAccurately convey the meaning and nuances of the original text using {target_lang} grammar and vocabulary. Preserve paragraph breaks, formatting, and proper names where appropriate.\nWhen the source and target languages differ, translate the prose rather than copying the source text unchanged.\nProduce only the {target_lang} translation, without language detection labels, explanations, or commentary.\n\nText to translate:\n{text}"
    )
}

fn language_code(language: &str) -> &'static str {
    match language {
        "English" => "en",
        "Korean" => "ko",
        "Japanese" => "ja",
        "Chinese (Simplified)" => "zh-Hans",
        "Chinese (Traditional)" => "zh-Hant",
        "Spanish" => "es",
        "French" => "fr",
        "German" => "de",
        "Russian" => "ru",
        _ => "auto",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct LanguageCase {
        name: String,
        text: String,
        source: Option<String>,
        document: Option<String>,
        target: Option<String>,
        expected: String,
    }

    #[test]
    fn shared_source_language_cases() {
        let cases: Vec<LanguageCase> =
            serde_json::from_str(include_str!("../../tests/fixtures/source-language.json"))
                .unwrap();

        for entry in cases {
            let source = entry.source.as_deref().unwrap_or("Auto Detect");
            let document = entry.document.as_deref().unwrap_or("Auto Detect");
            let target = entry.target.as_deref().unwrap_or("Korean");
            assert_eq!(
                resolve_source_language(source, &entry.text, document),
                entry.expected,
                "{}",
                entry.name
            );

            let prompt = create_formatted_prompt(source, target, &entry.text, document);
            assert!(prompt.ends_with(&format!("Text to translate:\n{}", entry.text)));
            assert!(prompt.contains(&format!("Produce only the {target} translation")));
            if entry.expected == "Auto Detect" {
                assert!(prompt.contains("Identify the source language from the text itself"));
            } else {
                assert!(prompt.contains(&format!("professional {} (", entry.expected)));
            }
            assert_eq!(
                prompt.contains("The source text is Japanese."),
                entry.expected == "Japanese" && target != "Japanese",
                "{}",
                entry.name
            );
        }
    }

    #[test]
    fn document_context_survives_chunk_boundaries_and_resume() {
        let text = "これは日本語の文章です。\n\n東京都\n\n第十二章";
        let document = resolve_source_language("Auto Detect", text, "Auto Detect");
        assert_eq!(document, "Japanese");
        for chunk in text.split("\n\n").skip(1) {
            assert_eq!(
                resolve_source_language("Auto Detect", chunk, document),
                "Japanese"
            );
            assert!(
                create_formatted_prompt("Auto Detect", "Korean", chunk, document)
                    .contains("Japanese (ja) to Korean (ko)")
            );
        }
    }
}
