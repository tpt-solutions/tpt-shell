// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Agent profiles from the manifest's `[agent]` section (spec §10.4).
//!
//! ```toml
//! [agent]
//! enabled = true
//!
//! [agent.profiles.assistant]
//! actions = ["click", "type", "read", "wait_for", "invoke"]
//! routes = ["app://tpt-app.localhost/*"]
//! commands = ["set_volume"]        # optional: default is every agent-flagged command
//! confirm = ["set_volume"]
//! rate_burst = 10
//! rate_per_second = 2
//! max_session_secs = 900
//! ```
//!
//! Nothing is enabled unless `enabled = true`. Unknown keys, unknown action names and
//! unusable route patterns are refused with `TS0110`, so a typo cannot widen a profile.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::Duration;

use toml::{Table, Value};
use tpt_shell_ipc::rate_limit::RateLimit;

use crate::Policy;

/// A profile is invalid. Fix: correct the key, name, action or pattern the message names.
pub const CODE_PROFILE: &str = "TS0110";

/// Every verb of the action API. `invoke` runs typed commands; the rest drive the page.
pub const ACTIONS: [&str; 9] = [
    "click", "focus", "type", "select", "key", "scroll", "wait_for", "read", "invoke",
];

const PROFILE_KEYS: [&str; 7] = [
    "actions",
    "routes",
    "commands",
    "confirm",
    "rate_burst",
    "rate_per_second",
    "max_session_secs",
];

/// A manifest profile problem, with its code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileError {
    pub message: String,
}

impl ProfileError {
    fn new(message: impl Into<String>) -> Self {
        ProfileError {
            message: message.into(),
        }
    }
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{CODE_PROFILE}: {}", self.message)
    }
}

impl std::error::Error for ProfileError {}

/// The URLs an agent may navigate to. Patterns are exact, or a prefix ending in `*`. An empty
/// list allows nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RouteRules {
    patterns: Vec<String>,
}

impl RouteRules {
    /// Builds the rules. A bare `*` is refused, because it would allow every URL.
    pub fn new(patterns: Vec<String>) -> Result<Self, ProfileError> {
        for p in &patterns {
            let prefix = p.strip_suffix('*').unwrap_or(p);
            if prefix.is_empty() || prefix.contains(char::is_whitespace) {
                return Err(ProfileError::new(format!(
                    "route pattern `{p}` is empty or matches everything"
                )));
            }
        }
        Ok(RouteRules { patterns })
    }

    /// Whether `url` is on the allow-list. A URL with whitespace or control characters never is.
    pub fn allows(&self, url: &str) -> bool {
        if url.is_empty() || url.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return false;
        }
        self.patterns.iter().any(|p| match p.strip_suffix('*') {
            Some(prefix) => url.starts_with(prefix),
            None => url == p,
        })
    }
}

/// One agent profile.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentProfile {
    pub name: String,
    /// Verbs of the action API this profile may use. `invoke` allows typed commands.
    pub actions: BTreeSet<String>,
    pub routes: RouteRules,
    /// Typed commands this profile may invoke. `None` means every agent-flagged command.
    pub commands: Option<BTreeSet<String>>,
    /// Commands that need the user's native confirmation before each call.
    pub confirm: BTreeSet<String>,
    pub rate: RateLimit,
    pub max_session: Duration,
}

impl AgentProfile {
    /// Whether the page action `verb` is allowed.
    pub fn allows_action(&self, verb: &str) -> bool {
        self.actions.contains(verb)
    }

    /// Whether typed command `name` may be invoked by this profile.
    pub fn allows_command(&self, name: &str) -> bool {
        self.actions.contains("invoke") && self.commands.as_ref().is_none_or(|c| c.contains(name))
    }

    /// The session policy this profile implies.
    pub fn policy(&self) -> Policy {
        Policy {
            confirm: self.confirm.clone(),
            rate: self.rate,
            max_session: self.max_session,
            routes: self.routes.clone(),
            invoke: self.actions.contains("invoke"),
        }
    }
}

/// The parsed `[agent]` section.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentConfig {
    pub enabled: bool,
    pub profiles: BTreeMap<String, AgentProfile>,
}

impl AgentConfig {
    /// The profile called `name`, when the agent is enabled and the profile exists.
    pub fn profile(&self, name: &str) -> Option<&AgentProfile> {
        if self.enabled {
            self.profiles.get(name)
        } else {
            None
        }
    }
}

/// Reads the `[agent]` section, given as the manifest keeps it (a TOML table). An absent
/// section gives a disabled config.
pub fn parse_agent_section(table: Option<&Table>) -> Result<AgentConfig, ProfileError> {
    let Some(table) = table else {
        return Ok(AgentConfig {
            enabled: false,
            profiles: BTreeMap::new(),
        });
    };
    check_keys(table, &["enabled", "profiles"], "agent")?;
    let enabled = match table.get("enabled") {
        None => false,
        Some(Value::Boolean(b)) => *b,
        Some(_) => return Err(ProfileError::new("agent.enabled must be true or false")),
    };
    let mut profiles = BTreeMap::new();
    if let Some(value) = table.get("profiles") {
        let Value::Table(list) = value else {
            return Err(ProfileError::new(
                "agent.profiles must be a table of profiles",
            ));
        };
        for (name, value) in list {
            let Value::Table(body) = value else {
                return Err(ProfileError::new(format!(
                    "agent.profiles.{name} must be a table"
                )));
            };
            profiles.insert(name.clone(), parse_profile(name, body)?);
        }
    }
    Ok(AgentConfig { enabled, profiles })
}

fn parse_profile(name: &str, body: &Table) -> Result<AgentProfile, ProfileError> {
    let at = |key: &str| format!("agent.profiles.{name}.{key}");
    if name.is_empty()
        || name.len() > 64
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(ProfileError::new(format!(
            "profile name `{name}` must be 1-64 ASCII letters, digits, '-' or '_'"
        )));
    }
    check_keys(body, &PROFILE_KEYS, &format!("agent.profiles.{name}"))?;

    let actions = string_set(body, "actions", &at("actions"))?.unwrap_or_default();
    for verb in &actions {
        if !ACTIONS.contains(&verb.as_str()) {
            return Err(ProfileError::new(format!(
                "`{verb}` in {} is not an action. Known actions: {}",
                at("actions"),
                ACTIONS.join(", ")
            )));
        }
    }
    let routes = RouteRules::new(
        string_set(body, "routes", &at("routes"))?
            .unwrap_or_default()
            .into_iter()
            .collect(),
    )?;
    let commands = string_set(body, "commands", &at("commands"))?;
    let confirm = string_set(body, "confirm", &at("confirm"))?.unwrap_or_default();
    let burst = integer(body, "rate_burst", &at("rate_burst"))?.unwrap_or(10);
    let per_second = integer(body, "rate_per_second", &at("rate_per_second"))?.unwrap_or(2);
    let max_session = integer(body, "max_session_secs", &at("max_session_secs"))?.unwrap_or(3600);
    if burst < 1 || per_second < 1 || max_session < 1 {
        return Err(ProfileError::new(format!(
            "agent.profiles.{name}: rate_burst, rate_per_second and max_session_secs must be at least 1"
        )));
    }
    Ok(AgentProfile {
        name: name.to_string(),
        actions,
        routes,
        commands,
        confirm,
        rate: RateLimit {
            burst: u32::try_from(burst).unwrap_or(u32::MAX),
            per_second: u32::try_from(per_second).unwrap_or(u32::MAX),
        },
        max_session: Duration::from_secs(u64::try_from(max_session).unwrap_or(u64::MAX)),
    })
}

fn check_keys(table: &Table, allowed: &[&str], section: &str) -> Result<(), ProfileError> {
    for key in table.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(ProfileError::new(format!(
                "unknown key `{key}` in {section}. Allowed: {}",
                allowed.join(", ")
            )));
        }
    }
    Ok(())
}

fn string_set(
    table: &Table,
    key: &str,
    at: &str,
) -> Result<Option<BTreeSet<String>>, ProfileError> {
    match table.get(key) {
        None => Ok(None),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::String(s) => Ok(s.clone()),
                _ => Err(ProfileError::new(format!("{at} must hold only strings"))),
            })
            .collect::<Result<BTreeSet<String>, ProfileError>>()
            .map(Some),
        Some(_) => Err(ProfileError::new(format!("{at} must be a list of strings"))),
    }
}

fn integer(table: &Table, key: &str, at: &str) -> Result<Option<i64>, ProfileError> {
    match table.get(key) {
        None => Ok(None),
        Some(Value::Integer(n)) => Ok(Some(*n)),
        Some(_) => Err(ProfileError::new(format!("{at} must be a whole number"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<AgentConfig, ProfileError> {
        let table: Table = text.parse().expect("test TOML");
        parse_agent_section(Some(&table))
    }

    #[test]
    fn an_absent_section_is_disabled() {
        let config = parse_agent_section(None).unwrap();
        assert!(!config.enabled);
        assert!(config.profile("anything").is_none());
    }

    #[test]
    fn a_profile_is_disabled_until_the_section_is_enabled() {
        let config = parse("[profiles.a]\nactions = [\"read\"]\n").unwrap();
        assert!(config.profile("a").is_none());
        let config = parse("enabled = true\n[profiles.a]\nactions = [\"read\"]\n").unwrap();
        assert!(config.profile("a").is_some());
    }

    #[test]
    fn a_full_profile_parses_with_its_limits() {
        let config = parse(
            r#"enabled = true
            [profiles.assistant]
            actions = ["click", "read", "invoke"]
            routes = ["app://tpt-app.localhost/*", "app://tpt-app.localhost/help.html"]
            commands = ["set_volume"]
            confirm = ["set_volume"]
            rate_burst = 5
            rate_per_second = 1
            max_session_secs = 120
            "#,
        )
        .unwrap();
        let p = config.profile("assistant").unwrap();
        assert!(p.allows_action("click"));
        assert!(!p.allows_action("type"));
        assert!(p.allows_command("set_volume"));
        assert!(!p.allows_command("delete_all"));
        assert_eq!(
            p.rate,
            RateLimit {
                burst: 5,
                per_second: 1
            }
        );
        assert_eq!(p.max_session, Duration::from_secs(120));
        let policy = p.policy();
        assert!(policy.invoke);
        assert!(policy.confirm.contains("set_volume"));
    }

    #[test]
    fn a_profile_without_invoke_offers_no_commands() {
        let config = parse("enabled = true\n[profiles.p]\nactions = [\"read\"]\n").unwrap();
        let p = config.profile("p").unwrap();
        assert!(!p.allows_command("set_volume"));
        assert!(!p.policy().invoke);
    }

    #[test]
    fn unknown_keys_and_actions_are_refused_with_the_code() {
        let err = parse("enabled = true\n[profiles.p]\nactionz = []\n").unwrap_err();
        assert!(err.to_string().starts_with(CODE_PROFILE));
        assert!(err.message.contains("actionz"));
        let err = parse("enabled = true\n[profiles.p]\nactions = [\"shell_exec\"]\n").unwrap_err();
        assert!(err.message.contains("shell_exec"));
        assert!(parse("enabled = true\nextra = 1\n").is_err());
    }

    #[test]
    fn bad_route_patterns_and_names_are_refused() {
        assert!(parse("enabled = true\n[profiles.p]\nroutes = [\"*\"]\n").is_err());
        assert!(parse("enabled = true\n[profiles.p]\nroutes = [\"\"]\n").is_err());
        assert!(parse("enabled = true\n[profiles.\"bad name\"]\n").is_err());
        assert!(parse("enabled = true\n[profiles.p]\nrate_burst = 0\n").is_err());
    }

    #[test]
    fn route_rules_allow_only_listed_urls() {
        let rules = RouteRules::new(vec!["app://tpt-app.localhost/*".into()]).unwrap();
        assert!(rules.allows("app://tpt-app.localhost/settings.html"));
        assert!(!rules.allows("https://example.com/"));
        assert!(!rules.allows("app://tpt-app.localhost/x y"));
        assert!(!rules.allows("app://tpt-app.localhost/\nx"));
        assert!(!RouteRules::default().allows("app://tpt-app.localhost/"));
        let exact = RouteRules::new(vec!["app://a/index.html".into()]).unwrap();
        assert!(exact.allows("app://a/index.html"));
        assert!(!exact.allows("app://a/index.html.evil"));
    }
}
