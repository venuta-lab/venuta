//! Language names in English, for the few places a language is named (a sentence to the
//! listener, a prompt to a language model).

/// The base of a tag ("pt-BR" is "pt"), in lower case.
pub fn base(tag: &str) -> String {
    tag.split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// The language's name in English, for the languages Venuta's lyrics come in most.
pub fn name(tag: &str) -> Option<&'static str> {
    Some(match base(tag).as_str() {
        "af" => "Afrikaans",
        "ar" => "Arabic",
        "as" => "Assamese",
        "bg" => "Bulgarian",
        "bn" => "Bengali",
        "ca" => "Catalan",
        "cs" => "Czech",
        "da" => "Danish",
        "de" => "German",
        "el" => "Greek",
        "en" => "English",
        "es" => "Spanish",
        "et" => "Estonian",
        "fa" => "Persian",
        "fi" => "Finnish",
        "fr" => "French",
        "gu" => "Gujarati",
        "he" => "Hebrew",
        "hi" => "Hindi",
        "hr" => "Croatian",
        "hu" => "Hungarian",
        "id" => "Indonesian",
        "it" => "Italian",
        "ja" => "Japanese",
        "kn" => "Kannada",
        "ko" => "Korean",
        "lt" => "Lithuanian",
        "lv" => "Latvian",
        "ml" => "Malayalam",
        "mr" => "Marathi",
        "ms" => "Malay",
        "nb" | "no" => "Norwegian",
        "ne" => "Nepali",
        "nl" => "Dutch",
        "or" => "Odia",
        "pa" => "Punjabi",
        "pl" => "Polish",
        "pt" => "Portuguese",
        "ro" => "Romanian",
        "ru" => "Russian",
        "sa" => "Sanskrit",
        "sk" => "Slovak",
        "sl" => "Slovenian",
        "sr" => "Serbian",
        "sv" => "Swedish",
        "sw" => "Swahili",
        "ta" => "Tamil",
        "te" => "Telugu",
        "th" => "Thai",
        "tl" | "fil" => "Filipino",
        "tr" => "Turkish",
        "uk" => "Ukrainian",
        "ur" => "Urdu",
        "vi" => "Vietnamese",
        "zh" => "Chinese",
        _ => return None,
    })
}
