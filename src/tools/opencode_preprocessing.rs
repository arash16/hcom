//! OpenCode launch preprocessing — sets environment variables for hcom integration.
//! Plugin management is handled separately in hooks/opencode.rs.

use std::collections::HashMap;
use std::sync::OnceLock;

/// OpenCode 2 hosts plugins in a shared background service that never sees
/// hcom's launch env; `--standalone` runs a private server per launch instead.
const STANDALONE_FLAG: &str = "--standalone";
const STANDALONE_MIN_MAJOR: u64 = 2;

fn opencode_permission_json() -> String {
    let prefix = crate::runtime_env::build_hcom_command();
    let bash = crate::hooks::common::SAFE_HCOM_COMMANDS
        .iter()
        .map(|command| (format!("{prefix} {command}*"), serde_json::json!("allow")))
        .collect();
    serde_json::Value::Object(serde_json::Map::from_iter([(
        "bash".to_string(),
        serde_json::Value::Object(bash),
    )]))
    .to_string()
}

/// Preprocess environment variables for an OpenCode-family launch.
///
/// Sets:
/// - App-specific permission override: Auto-approve safe hcom bash commands when enabled
/// - `HCOM_NAME`: Instance name for plugin diagnostics (set before identity binding)
pub fn preprocess_opencode_env(
    env: &mut HashMap<String, String>,
    tool: &str,
    instance_name: &str,
    auto_approve: bool,
) {
    if auto_approve {
        let key = if tool == "kilo" {
            "KILO_PERMISSION"
        } else {
            "OPENCODE_PERMISSION"
        };
        env.insert(key.to_string(), opencode_permission_json());
    }
    env.insert("HCOM_NAME".to_string(), instance_name.to_string());
}

/// Preprocess OpenCode launch args: add `--standalone` on OpenCode 2+.
pub fn preprocess_opencode_args(args: &[String]) -> Vec<String> {
    add_standalone(args, opencode_supports_standalone())
}

/// Leaves the args alone when the user already chose a server.
fn add_standalone(args: &[String], supported: bool) -> Vec<String> {
    let chosen = args
        .iter()
        .any(|arg| arg == STANDALONE_FLAG || arg == "--server" || arg.starts_with("--server="));
    let mut result = args.to_vec();
    if supported && !chosen {
        result.insert(0, STANDALONE_FLAG.to_string());
    }
    result
}

fn parse_opencode_major_version(output: &str) -> Option<u64> {
    output
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .find(|token| token.matches('.').count() >= 2)
        .and_then(|token| token.split('.').next()?.parse().ok())
}

fn opencode_supports_standalone() -> bool {
    static CACHE: OnceLock<bool> = OnceLock::new();
    *CACHE.get_or_init(|| {
        let output = match crate::terminal::executable_command("opencode")
            .arg("--version")
            .output()
        {
            Ok(output) => output,
            Err(e) => {
                crate::log::log_warn(
                    "opencode",
                    "opencode.version_failed",
                    &format!("could not run opencode --version; skipping {STANDALONE_FLAG}: {e}"),
                );
                return false;
            }
        };
        parse_opencode_major_version(&String::from_utf8_lossy(&output.stdout))
            .is_some_and(|major| major >= STANDALONE_MIN_MAJOR)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preprocess_sets_permission() {
        let mut env = HashMap::new();
        preprocess_opencode_env(&mut env, "opencode", "luna", true);
        let perm = env.get("OPENCODE_PERMISSION").unwrap();
        let prefix = crate::runtime_env::build_hcom_command();
        assert!(perm.contains(&format!("{prefix} send*")));
        assert!(!perm.contains(&format!("\"{prefix} *\"")));
        assert!(!perm.contains("hcom kill"));
    }

    #[test]
    fn test_preprocess_skips_permission_when_disabled() {
        let mut env = HashMap::new();
        preprocess_opencode_env(&mut env, "opencode", "luna", false);
        assert!(!env.contains_key("OPENCODE_PERMISSION"));
    }

    #[test]
    fn test_preprocess_sets_hcom_name() {
        let mut env = HashMap::new();
        preprocess_opencode_env(&mut env, "opencode", "nova", true);
        assert_eq!(env.get("HCOM_NAME").unwrap(), "nova");
    }

    #[test]
    fn test_preprocess_overwrites_existing() {
        let mut env = HashMap::new();
        env.insert("HCOM_NAME".to_string(), "old".to_string());
        preprocess_opencode_env(&mut env, "opencode", "nova", true);
        assert_eq!(env.get("HCOM_NAME").unwrap(), "nova");
    }

    #[test]
    fn test_preprocess_kilo_sets_kilo_permission() {
        let mut env = HashMap::new();
        preprocess_opencode_env(&mut env, "kilo", "luna", true);
        assert!(env.contains_key("KILO_PERMISSION"));
        assert!(!env.contains_key("OPENCODE_PERMISSION"));
    }

    #[test]
    fn test_parse_opencode_major_version() {
        assert_eq!(parse_opencode_major_version("opencode v2.0.15\n"), Some(2));
        assert_eq!(parse_opencode_major_version("1.14.48"), Some(1));
        assert_eq!(parse_opencode_major_version("opencode"), None);
    }

    #[test]
    fn test_add_standalone_when_supported() {
        let args = vec!["--model".to_string(), "a/b".to_string()];
        assert_eq!(
            add_standalone(&args, true),
            ["--standalone", "--model", "a/b"]
        );
        assert_eq!(add_standalone(&args, false), args);
    }

    #[test]
    fn test_add_standalone_respects_chosen_server() {
        for args in [
            vec!["--standalone".to_string()],
            vec!["--server".to_string(), "http://x".to_string()],
            vec!["--server=http://x".to_string()],
        ] {
            assert_eq!(add_standalone(&args, true), args);
        }
    }

    #[test]
    fn test_permission_json_is_valid() {
        let parsed: serde_json::Value =
            serde_json::from_str(&opencode_permission_json()).expect("valid JSON");
        let prefix = crate::runtime_env::build_hcom_command();
        assert!(parsed["bash"][format!("{prefix} send*")].is_string());
    }
}
