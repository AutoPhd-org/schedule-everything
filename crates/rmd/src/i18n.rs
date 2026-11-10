//! User-facing text in English, Simplified Chinese, Spanish, or Korean.
//!
//! The active language comes from `REMINDER_LANG`, then `[settings] language`
//! in the active `settings.toml`, then English. Source strings are the English
//! text; [`t`] looks them up in the active language's translation table
//! (`i18n_table.rs` for Chinese, `i18n_table_es.rs`, `i18n_table_ko.rs`), and
//! falls back to the English.

use serde_json::Value;

use crate::i18n_table::ZH_TRANSLATIONS;
use crate::i18n_table_es::ES_TRANSLATIONS;
use crate::i18n_table_ko::KO_TRANSLATIONS;

/// Language codes `[settings] language` accepts, in the order pickers offer them.
pub const LANGUAGES: [&str; 4] = ["en", "zh", "es", "ko"];

/// Active language: `"en"`, `"zh"`, `"es"`, or `"ko"`.
pub fn get_language() -> &'static str {
    if let Ok(env_lang) = std::env::var("REMINDER_LANG")
        && !env_lang.is_empty()
    {
        return normalize_language(&env_lang).unwrap_or("en");
    }
    let settings_path = crate::paths::runtime().settings_path;
    if settings_path.exists()
        && let Ok(config) = crate::config::load_toml_file(&settings_path)
        && let Some(Value::Object(settings)) = config.get("settings")
        && let Some(Value::String(lang)) = settings.get("language")
    {
        return normalize_language(lang).unwrap_or("en");
    }
    "en"
}

/// The language code a setting or locale names, if it is one we have: `zh`,
/// `zh-CN`, `cn`, `chinese`, `中文`; `es`, `es_MX`, `spanish`, `español`;
/// `ko`, `ko-KR`, `kr`, `korean`, `한국어`; `en`, `en-GB`, `english`.
pub fn normalize_language(lang: &str) -> Option<&'static str> {
    let lowered = lang.trim().to_lowercase();
    let base = lowered.split(['-', '_', '.']).next().unwrap_or("");
    match base {
        "zh" | "cn" | "chinese" | "中文" | "简体中文" => Some("zh"),
        "es" | "spanish" | "español" | "espanol" => Some("es"),
        "ko" | "kr" | "korean" | "한국어" => Some("ko"),
        "en" | "english" => Some("en"),
        _ => None,
    }
}

fn table(lang: &str) -> Option<&'static [(&'static str, &'static str)]> {
    match lang {
        "zh" => Some(ZH_TRANSLATIONS),
        "es" => Some(ES_TRANSLATIONS),
        "ko" => Some(KO_TRANSLATIONS),
        _ => None,
    }
}

/// Translation of an English source string into `lang`, if there is one.
pub fn lookup(lang: &str, text: &str) -> Option<&'static str> {
    let table = table(lang)?;
    table.binary_search_by(|(key, _)| key.as_bytes().cmp(text.as_bytes())).ok().map(|i| table[i].1)
}

/// Chinese translation of an English source string, if there is one.
pub fn zh(text: &str) -> Option<&'static str> {
    lookup("zh", text)
}

/// Translate to the active language (Python `_t`).
pub fn t(text: &str) -> String {
    lookup(get_language(), text).unwrap_or(text).to_string()
}

/// Python `str.format(**kwargs)` for `{name}` placeholders (with `{{`/`}}`).
pub fn format(template: &str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                out.push('}');
            }
            '{' => {
                let mut name = String::new();
                let mut closed = false;
                for n in chars.by_ref() {
                    if n == '}' {
                        closed = true;
                        break;
                    }
                    name.push(n);
                }
                match args.iter().find(|(k, _)| *k == name) {
                    Some((_, value)) if closed => out.push_str(&value.to_string()),
                    _ => {
                        out.push('{');
                        out.push_str(&name);
                        if closed {
                            out.push('}');
                        }
                    }
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Translate, then fill `{name}` placeholders: `tf("Task: {name}", &[("name", &x)])`.
pub fn tf(text: &str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    format(&t(text), args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_sorted_for_binary_search() {
        for lang in ["zh", "es", "ko"] {
            let table = table(lang).unwrap();
            assert!(table.windows(2).all(|w| w[0].0.as_bytes() < w[1].0.as_bytes()), "{lang}");
        }
        assert_eq!(zh("rmd CLI"), Some("rmd 命令行工具"));
        assert_eq!(zh("no such string"), None);
        assert_eq!(lookup("en", "rmd CLI"), None);
    }

    /// Spanish and Korean translate exactly the strings Chinese does, and keep
    /// each one's placeholders and surrounding whitespace.
    #[test]
    fn every_language_translates_the_same_strings() {
        fn placeholders(text: &str) -> Vec<String> {
            let mut names: Vec<String> = text
                .split('{')
                .skip(1)
                .filter_map(|part| part.split_once('}').map(|(name, _)| name.to_string()))
                .collect();
            names.sort();
            names
        }
        let keys: Vec<&str> = ZH_TRANSLATIONS.iter().map(|(key, _)| *key).collect();
        for (lang, table) in [("es", ES_TRANSLATIONS), ("ko", KO_TRANSLATIONS)] {
            assert_eq!(table.iter().map(|(key, _)| *key).collect::<Vec<_>>(), keys, "{lang}");
            for (key, text) in table {
                assert_eq!(placeholders(key), placeholders(text), "{lang}: {key}");
                assert_eq!(key.len() - key.trim_start().len(), text.len() - text.trim_start().len(), "{lang}: {key}");
                assert_eq!(key.ends_with(' '), text.ends_with(' '), "{lang}: {key}");
                assert_eq!(key.matches('\n').count(), text.matches('\n').count(), "{lang}: {key}");
            }
        }
    }

    #[test]
    fn normalizes_language_names() {
        for (value, code) in [
            ("zh", "zh"),
            ("zh_CN.UTF-8", "zh"),
            ("Chinese", "zh"),
            ("中文", "zh"),
            ("es", "es"),
            ("es-MX", "es"),
            ("Español", "es"),
            ("ko", "ko"),
            ("ko_KR", "ko"),
            ("kr", "ko"),
            ("한국어", "ko"),
            ("en-GB", "en"),
        ] {
            assert_eq!(normalize_language(value), Some(code), "{value}");
        }
        assert_eq!(normalize_language("fr"), None);
        assert_eq!(normalize_language(""), None);
    }

    #[test]
    fn format_fills_placeholders() {
        let count = 3;
        assert_eq!(format("{count} of {{x}} {missing}", &[("count", &count)]), "3 of {x} {missing}");
    }
}

#[cfg(test)]
mod language_tests {
    use super::*;
    use crate::test_support::TestEnv;

    #[test]
    fn default_language_is_english() {
        let _env = TestEnv::new();
        assert_eq!(get_language(), "en");
        assert_eq!(t("📋 No tasks found"), "📋 No tasks found");
    }

    #[test]
    fn env_variable_overrides_settings() {
        let mut env = TestEnv::new();
        env.write("user_config_0/settings.toml", "[settings]\nlanguage = \"en\"\n");
        for value in ["zh", "cn", "Chinese", "ZH"] {
            env.set_var("REMINDER_LANG", value);
            assert_eq!(get_language(), "zh", "{value}");
        }
        assert_eq!(t("📋 No tasks found"), "📋 未找到任务");
        env.set_var("REMINDER_LANG", "ko_KR.UTF-8");
        assert_eq!(get_language(), "ko");
        env.set_var("REMINDER_LANG", "en");
        assert_eq!(get_language(), "en");
    }

    fn es_text(key: &str) -> &'static str {
        lookup("es", key).unwrap()
    }

    #[test]
    fn settings_file_selects_the_language() {
        let env = TestEnv::new();
        env.write("user_config_0/settings.toml", "[settings]\nlanguage = \"zh\"\n");
        assert_eq!(get_language(), "zh");
        env.write("user_config_0/settings.toml", "[settings]\nlanguage = \"es\"\n");
        assert_eq!(get_language(), "es");
        assert_eq!(t("📋 No tasks found"), es_text("📋 No tasks found"));
        env.write("user_config_0/settings.toml", "[settings]\nlanguage = \"korean\"\n");
        assert_eq!(get_language(), "ko");
        env.write("user_config_0/settings.toml", "[settings]\nlanguage = \"fr\"\n");
        assert_eq!(get_language(), "en");
        env.write("user_config_0/settings.toml", "[settings]\nlanguage = \"en\"\n");
        assert_eq!(get_language(), "en");
        env.write("user_config_0/settings.toml", "not toml [");
        assert_eq!(get_language(), "en");
    }

    #[test]
    fn dynamic_formatting_and_fallback() {
        let mut env = TestEnv::new();
        let template = "✅ Task '{task_description}' updated! Priority changed from {old_priority} to {priority}";
        let args: [(&str, &dyn std::fmt::Display); 3] =
            [("task_description", &"Testing i18n"), ("old_priority", &3), ("priority", &7)];
        env.set_var("REMINDER_LANG", "en");
        assert_eq!(tf(template, &args), "✅ Task 'Testing i18n' updated! Priority changed from 3 to 7");
        env.set_var("REMINDER_LANG", "zh");
        assert_eq!(tf(template, &args), "✅ 任务 'Testing i18n' 已更新！优先级从 3 修改为 7");
        let missing = "This string is not inside our ZH_TRANSLATIONS dictionary!";
        assert_eq!(t(missing), missing);
    }
}
