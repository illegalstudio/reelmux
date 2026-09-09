use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Language {
    pub name: String,
    pub alpha2: String,
    pub alpha3: String,
    pub bibliographic: String,
}

pub fn languages() -> &'static [Language] {
    static LANGUAGES: OnceLock<Vec<Language>> = OnceLock::new();
    LANGUAGES.get_or_init(|| {
        serde_json::from_str(include_str!("../data/languages.json"))
            .expect("the bundled language table must be valid")
    })
}

/// Normalize ISO language codes using the table adapted from MP42Foundation.
pub fn normalize(code: &str) -> Option<&'static str> {
    let code = code.trim().to_ascii_lowercase();
    if code.is_empty() {
        return Some("und");
    }
    languages()
        .iter()
        .find(|entry| entry.alpha3 == code || entry.alpha2 == code || entry.bibliographic == code)
        .map(|entry| entry.alpha3.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_short_and_bibliographic_codes() {
        assert_eq!(normalize(" IT "), Some("ita"));
        assert_eq!(normalize("ger"), Some("deu"));
        assert_eq!(normalize("fre"), Some("fra"));
        assert_eq!(normalize(""), Some("und"));
        assert_eq!(normalize("not-a-language"), None);
        assert!(languages().len() > 400);
    }
}
