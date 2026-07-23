//! Optional env loading for build-time config (forms, endpoints, …).
//!
//! Priority (lowest → highest; process env always wins):
//! 1. `[env]` in statica.toml
//! 2. `.env`
//! 3. `.dev.vars`

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// `[env]` — inline vars + optional `.env` / `.dev.vars` loading.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EnvConfig {
    /// Load `.env` and `.dev.vars` from the config directory when true.
    #[serde(default = "default_true")]
    pub load_files: bool,
    /// Inline build-time env vars from statica.toml.
    #[serde(flatten)]
    pub vars: HashMap<String, String>,
}

fn default_true() -> bool {
    true
}

impl Default for EnvConfig {
    fn default() -> Self {
        Self {
            load_files: true,
            vars: HashMap::new(),
        }
    }
}

/// Apply statica env layers. Does not override variables already in the process env.
pub fn apply(config_dir: &Path, cfg: &EnvConfig) -> Result<()> {
    let process = std::env::vars().map(|(k, _)| k).collect::<HashSet<_>>();

    apply_vars(&cfg.vars, &process);
    if !cfg.load_files {
        return Ok(());
    }

    let dot_env = config_dir.join(".env");
    if dot_env.is_file() {
        let vars = read_env_file(&dot_env)
            .with_context(|| format!("failed to read {}", dot_env.display()))?;
        apply_vars(&vars, &process);
    }

    let dev_vars = config_dir.join(".dev.vars");
    if dev_vars.is_file() {
        let vars = read_env_file(&dev_vars)
            .with_context(|| format!("failed to read {}", dev_vars.display()))?;
        apply_vars(&vars, &process);
    }

    Ok(())
}

fn apply_vars(vars: &HashMap<String, String>, process: &HashSet<String>) {
    for (key, value) in vars {
        if process.contains(key) {
            continue;
        }
        std::env::set_var(key, value);
    }
}

/// Parse a dotenv-style file (`KEY=VALUE`, `#` comments, optional quotes).
pub fn read_env_file(path: &Path) -> Result<HashMap<String, String>> {
    let text = fs::read_to_string(path)?;
    Ok(parse_env_text(&text))
}

fn parse_env_text(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim();
        let Some((key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        out.insert(key.to_string(), parse_env_value(raw_value.trim()));
    }
    out
}

fn parse_env_value(raw: &str) -> String {
    if raw.len() >= 2 {
        let bytes = raw.as_bytes();
        let quote = bytes[0];
        if (quote == b'"' || quote == b'\'') && bytes[bytes.len() - 1] == quote {
            return raw[1..raw.len() - 1].to_string();
        }
    }
    raw.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    fn env_lock() -> MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn parse_env_text_basic() {
        let vars = parse_env_text(
            r#"
# comment
FORMS_CONTACT_ID=xyzabc
FORMS_ENDPOINT="https://formspree.io/f/{id}"
export EMPTY=
"#,
        );
        assert_eq!(vars.get("FORMS_CONTACT_ID").map(String::as_str), Some("xyzabc"));
        assert_eq!(
            vars.get("FORMS_ENDPOINT").map(String::as_str),
            Some("https://formspree.io/f/{id}")
        );
        assert_eq!(vars.get("EMPTY").map(String::as_str), Some(""));
    }

    #[test]
    fn apply_respects_process_env_and_file_priority() {
        let _lock = env_lock();
        std::env::set_var("STATICA_TEST_KEEP", "from-process");
        std::env::remove_var("STATICA_TEST_CONFIG");
        std::env::remove_var("STATICA_TEST_DOTENV");
        std::env::remove_var("STATICA_TEST_DEVVARS");

        let dir = std::env::temp_dir().join(format!(
            "statica-env-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(".env"),
            "STATICA_TEST_CONFIG=from-config\nSTATICA_TEST_DOTENV=from-dotenv\n",
        )
        .unwrap();
        fs::write(
            dir.join(".dev.vars"),
            "STATICA_TEST_DOTENV=from-devvars\nSTATICA_TEST_DEVVARS=from-devvars\n",
        )
        .unwrap();

        let cfg = EnvConfig {
            load_files: true,
            vars: HashMap::from([("STATICA_TEST_CONFIG".into(), "from-toml".into())]),
        };
        apply(&dir, &cfg).unwrap();

        assert_eq!(
            std::env::var("STATICA_TEST_KEEP").unwrap(),
            "from-process"
        );
        assert_eq!(
            std::env::var("STATICA_TEST_CONFIG").unwrap(),
            "from-config"
        );
        assert_eq!(
            std::env::var("STATICA_TEST_DOTENV").unwrap(),
            "from-devvars"
        );
        assert_eq!(
            std::env::var("STATICA_TEST_DEVVARS").unwrap(),
            "from-devvars"
        );

        std::env::remove_var("STATICA_TEST_KEEP");
        std::env::remove_var("STATICA_TEST_CONFIG");
        std::env::remove_var("STATICA_TEST_DOTENV");
        std::env::remove_var("STATICA_TEST_DEVVARS");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn load_files_false_skips_dotenv() {
        let _lock = env_lock();
        std::env::remove_var("STATICA_TEST_SKIP_FILE");

        let dir = std::env::temp_dir().join(format!(
            "statica-env-skip-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(".env"), "STATICA_TEST_SKIP_FILE=from-dotenv\n").unwrap();

        let cfg = EnvConfig {
            load_files: false,
            vars: HashMap::new(),
        };
        apply(&dir, &cfg).unwrap();
        assert!(std::env::var("STATICA_TEST_SKIP_FILE").is_err());

        let _ = fs::remove_dir_all(dir);
    }
}
