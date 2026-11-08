//! Bilingual (English / Simplified Chinese) user-facing text.
//!
//! The active language comes from `REMINDER_LANG`, then `[settings] language`
//! in the active `settings.toml`, then English. Source strings are the English
//! text; [`t`] looks them up in the translation table (`i18n_table.rs`).

use serde_json::Value;

use crate::i18n_table::ZH_TRANSLATIONS;

/// Active language: `"en"` or `"zh"`.
pub fn get_language() -> &'static str {
    if let Ok(env_lang) = std::env::var("REMINDER_LANG")
        && !env_lang.is_empty()
    {
        return if is_chinese(&env_lang) { "zh" } else { "en" };
    }
    let settings_path = crate::paths::runtime().settings_path;
    if settings_path.exists()
        && let Ok(config) = crate::config::load_toml_file(&settings_path)
        && let Some(Value::Object(settings)) = config.get("settings")
        && let Some(Value::String(lang)) = settings.get("language")
        && is_chinese(lang)
    {
        return "zh";
    }
    "en"
}

fn is_chinese(lang: &str) -> bool {
    matches!(lang.to_lowercase().as_str(), "zh" | "cn" | "chinese")
}

/// Chinese translation of an English source string, if there is one.
pub fn zh(text: &str) -> Option<&'static str> {
    ZH_TRANSLATIONS.binary_search_by(|(key, _)| key.as_bytes().cmp(text.as_bytes())).ok().map(|i| ZH_TRANSLATIONS[i].1)
}

/// Translate to the active language (Python `_t`).
pub fn t(text: &str) -> String {
    if get_language() == "zh"
        && let Some(translated) = zh(text)
    {
        return translated.to_string();
    }
    text.to_string()
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
    fn table_is_sorted_for_binary_search() {
        assert!(ZH_TRANSLATIONS.windows(2).all(|w| w[0].0.as_bytes() < w[1].0.as_bytes()));
        assert_eq!(zh("rmd CLI"), Some("rmd 命令行工具"));
        assert_eq!(zh("no such string"), None);
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
        env.set_var("REMINDER_LANG", "en");
        assert_eq!(get_language(), "en");
    }

    #[test]
    fn settings_file_selects_the_language() {
        let env = TestEnv::new();
        env.write("user_config_0/settings.toml", "[settings]\nlanguage = \"zh\"\n");
        assert_eq!(get_language(), "zh");
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
