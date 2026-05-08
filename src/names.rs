#![allow(dead_code)]

use anyhow::{anyhow, Result};

use crate::tmux;

// Keys are exposed both as `pub const &str` (for normal use) and via
// macro_rules! aliases (for use inside `concat!`, which only accepts literals).
// Keep both arms in sync.
macro_rules! key_name { () => { "@tt-name" }; }
macro_rules! key_agent { () => { "@tt-agent" }; }
macro_rules! key_access { () => { "@tt-access" }; }
macro_rules! key_launched_at { () => { "@tt-launched-at" }; }
macro_rules! key_cwd { () => { "@tt-cwd" }; }
pub(crate) use {key_access, key_agent, key_cwd, key_launched_at, key_name};

pub const KEY_NAME: &str = key_name!();
pub const KEY_AGENT: &str = key_agent!();
pub const KEY_ACCESS: &str = key_access!();
pub const KEY_LAUNCHED_AT: &str = key_launched_at!();
pub const KEY_CWD: &str = key_cwd!();

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Registered {
    pub name: Option<String>,
    pub agent: Option<String>,
    pub access: Option<String>,
    pub launched_at: Option<String>,
    pub cwd: Option<String>,
}

pub fn get(pane_id: &str, key: &str) -> Result<Option<String>> {
    let format = format!("#{{{key}}}");
    let args = ["display", "-p", "-t", pane_id, format.as_str()];
    let value = tmux::run_checked(&args)?.trim().to_owned();

    Ok((!value.is_empty()).then_some(value))
}

pub fn set(pane_id: &str, key: &str, value: &str) -> Result<()> {
    if key == KEY_NAME {
        let existing = list_panes_with_name(value)?;
        for other in &existing {
            if other != pane_id {
                return Err(anyhow!(
                    "name '{value}' is already used by pane {other}; pick a different name or kill the existing pane"
                ));
            }
        }
    }

    let args = ["set-option", "-p", "-t", pane_id, key, value];
    tmux::run_checked(&args)?;

    Ok(())
}

pub fn unset(pane_id: &str, key: &str) -> Result<()> {
    let args = ["set-option", "-p", "-u", "-t", pane_id, key];
    tmux::run_checked(&args)?;

    Ok(())
}

pub fn find_pane_by_name(name: &str) -> Result<Option<String>> {
    let pane_ids = list_panes_with_name(name)?;

    match pane_ids.len() {
        0 => Ok(None),
        1 => Ok(Some(pane_ids.into_iter().next().unwrap())),
        _ => Err(anyhow!(
            "name '{}' resolves to multiple panes: {}; use a pane id (%N) explicitly",
            name,
            pane_ids.join(", ")
        )),
    }
}

fn list_panes_with_name(name: &str) -> Result<Vec<String>> {
    let format = format!("#{{pane_id}}\t#{{{}}}", KEY_NAME);
    let args = ["list-panes", "-a", "-F", format.as_str()];
    let panes = tmux::run_checked(&args)?;

    Ok(parse_panes_with_name(&panes, name))
}

fn parse_panes_with_name(output: &str, name: &str) -> Vec<String> {
    let mut matches = Vec::new();
    for line in output.lines() {
        if let Some((pane_id, pane_name)) = line.split_once('\t') {
            if pane_name == name {
                matches.push(pane_id.to_owned());
            }
        }
    }
    matches
}

pub fn read(pane_id: &str) -> Result<Registered> {
    const FIELD_SEP: char = '\x1f';
    const FORMAT: &str = concat!(
        "#{", key_name!(), "}\x1f",
        "#{", key_agent!(), "}\x1f",
        "#{", key_access!(), "}\x1f",
        "#{", key_launched_at!(), "}\x1f",
        "#{", key_cwd!(), "}",
    );
    let raw = tmux::run_checked(&["display-message", "-p", "-t", pane_id, FORMAT])?;
    let line = raw.trim_end_matches('\n');
    let mut parts = line.splitn(5, FIELD_SEP);
    let name = parts.next().and_then(non_empty_owned);
    let agent = parts.next().and_then(non_empty_owned);
    let access = parts.next().and_then(non_empty_owned);
    let launched_at = parts.next().and_then(non_empty_owned);
    let cwd = parts.next().and_then(non_empty_owned);
    Ok(Registered { name, agent, access, launched_at, cwd })
}

fn non_empty_owned(s: &str) -> Option<String> {
    let trimmed = s.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_returns_empty_when_no_matches() {
        let output = "%1\tfoo\n%2\tbar\n";
        let matches = parse_panes_with_name(output, "baz");
        assert!(matches.is_empty());
    }

    #[test]
    fn parse_returns_single_match() {
        let output = "%1\tfoo\n%2\tbar\n%3\tbaz\n";
        let matches = parse_panes_with_name(output, "bar");
        assert_eq!(matches, vec!["%2".to_owned()]);
    }

    #[test]
    fn parse_returns_all_matches_for_duplicates() {
        let output = "%1\tdup\n%2\tother\n%3\tdup\n%4\tdup\n";
        let matches = parse_panes_with_name(output, "dup");
        assert_eq!(
            matches,
            vec!["%1".to_owned(), "%3".to_owned(), "%4".to_owned()]
        );
    }

    #[test]
    fn parse_skips_lines_without_tab() {
        let output = "no-tab-line\n%1\tname\n";
        let matches = parse_panes_with_name(output, "name");
        assert_eq!(matches, vec!["%1".to_owned()]);
    }

    #[test]
    fn parse_skips_panes_with_empty_names_when_searching_non_empty() {
        let output = "%1\t\n%2\tname\n%3\t\n";
        let matches = parse_panes_with_name(output, "name");
        assert_eq!(matches, vec!["%2".to_owned()]);
    }

    #[test]
    fn parse_handles_empty_output() {
        let matches = parse_panes_with_name("", "anything");
        assert!(matches.is_empty());
    }
}
