use std::time::Duration;

use mesh_core::acl::{self, ACL, AccessStrategy, Listener};

#[derive(Debug, Clone)]
pub enum PathFragment {
    String(String),
    Wildcard,
}
pub type PathFragments = Vec<PathFragment>;

pub fn fragments_from_str(v: &str) -> PathFragments {
    v.split('/')
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s == "*" {
                PathFragment::Wildcard
            } else {
                PathFragment::String(s.to_string())
            }
        })
        .collect()
}

pub fn get_timeout<'a, T: Iterator<Item = &'a (PathFragments, Duration)>>(
    rules: T,
    path: &str,
    default: Duration,
) -> Duration {
    let fragments = fragments_from_str(path);
    let d = rules
        .filter(|(rule, _)| rule.len() == fragments.len())
        .find(|(rule, _)| {
            rule.iter()
                .zip(&fragments)
                .all(|(rule_frag, path_frag)| match (rule_frag, path_frag) {
                    (PathFragment::Wildcard, _) => true,
                    (PathFragment::String(r), PathFragment::String(p)) => r.eq(p),
                    _ => false,
                })
        })
        .map_or(default, |(_, dur)| *dur);
    d
}

pub fn get_access_strategy(acl: &ACL, listener: &Listener, path: &str) -> AccessStrategy {
    let rules = acl
        .rules
        .iter()
        .filter(|rule| rule.listener == *listener)
        .map(|rule| (fragments_from_str(&rule.path), rule.strategy.clone()));
    let fragments = fragments_from_str(path);
    let strategy = rules
        .filter(|(rule, _)| rule.len() == fragments.len())
        .find(|(rule, _)| {
            rule.iter()
                .zip(&fragments)
                .all(|(rule_frag, path_frag)| match (rule_frag, path_frag) {
                    (PathFragment::Wildcard, _) => true,
                    (PathFragment::String(r), PathFragment::String(p)) => r.eq(p),
                    _ => false,
                })
        })
        .map_or(acl.default.clone(), |(_, strat)| strat.clone());
    strategy
}
