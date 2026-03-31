#[derive(Debug, Clone)]
pub struct Language {
    pub code: &'static str,
    pub name: &'static str,
}

const LANGUAGES: &[Language] = &[
    Language { code: "de", name: "German" },
    Language { code: "en", name: "English" },
    Language { code: "fr", name: "French" },
    Language { code: "es", name: "Spanish" },
    Language { code: "pt", name: "Portuguese" },
    Language { code: "it", name: "Italian" },
    Language { code: "nl", name: "Dutch" },
    Language { code: "pl", name: "Polish" },
    Language { code: "ru", name: "Russian" },
    Language { code: "ja", name: "Japanese" },
    Language { code: "zh", name: "Chinese" },
];

pub fn all_languages() -> Vec<Language> {
    LANGUAGES.to_vec()
}

pub fn all_languages_except(exclude_code: &str) -> Vec<Language> {
    LANGUAGES
        .iter()
        .filter(|l| l.code != exclude_code)
        .cloned()
        .collect()
}

pub fn resolve_language(input: &str) -> Option<Language> {
    let lower = input.to_lowercase();
    LANGUAGES
        .iter()
        .find(|l| l.code == lower || l.name.to_lowercase() == lower)
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_by_code() {
        let lang = resolve_language("pt").unwrap();
        assert_eq!(lang.code, "pt");
        assert_eq!(lang.name, "Portuguese");
    }

    #[test]
    fn resolve_by_name_case_insensitive() {
        let lang = resolve_language("portuguese").unwrap();
        assert_eq!(lang.code, "pt");
    }

    #[test]
    fn resolve_unknown_returns_none() {
        assert!(resolve_language("klingon").is_none());
    }

    #[test]
    fn all_languages_returns_11() {
        assert_eq!(all_languages().len(), 11);
    }

    #[test]
    fn all_languages_excludes_filters_one() {
        let filtered = all_languages_except("de");
        assert_eq!(filtered.len(), 10);
        assert!(filtered.iter().all(|l| l.code != "de"));
    }
}
