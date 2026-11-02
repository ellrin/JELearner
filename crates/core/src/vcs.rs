//! Pure helpers for Git sync: commit messages and a last-line secret check.
//! The git commands themselves run in the server.

/// A readable commit message from the changed paths, e.g.
/// `Update lessons (2026-11-02-ai-chips); vocab (my-words-ja)`.
pub fn commit_message(paths: &[String]) -> String {
    use std::collections::BTreeMap;
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for p in paths {
        let parts: Vec<&str> = p.split('/').collect();
        let stem = |s: &str| s.rsplit_once('.').map(|(a, _)| a).unwrap_or(s).to_string();
        let (group, item) = match parts.as_slice() {
            
            
            [top, .., file] => (top.to_string(), stem(file)),
            [file] => ("workspace".into(), stem(file)),
            [] => continue,
        };
        let items = groups.entry(group).or_default();
        if !item.is_empty() && !items.contains(&item) {
            items.push(item);
        }
    }
    if groups.is_empty() {
        return "Update workspace".into();
    }
    let body = groups
        .into_iter()
        .map(|(g, items)| {
            if items.is_empty() {
                g
            } else if items.len() > 4 {
                format!("{g} ({}, +{})", items[..3].join(", "), items.len() - 3)
            } else {
                format!("{g} ({})", items.join(", "))
            }
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!("Update {body}")
}

/// Return a label when a line looks like it contains a credential.
pub fn secret_like(line: &str) -> Option<&'static str> {
    let run_after = |marker: &str, min: usize| {
        line.match_indices(marker).any(|(i, _)| {
            line[i + marker.len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .count()
                >= min
        })
    };
    if run_after("sk-ant-", 20) {
        return Some("Anthropic API key");
    }
    if run_after("ghp_", 30) || run_after("github_pat_", 20) {
        return Some("GitHub token");
    }
    if line.contains("PRIVATE KEY-----") {
        return Some("private key");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_paths_into_a_message() {
        let m = commit_message(&[
            "lessons/ja/2026-11-02-ai.md".into(),
            "vocab/my-words-ja.csv".into(),
            "vocab/my-words-ja.csv".into(),
            "workspace.toml".into(),
        ]);
        assert_eq!(m, "Update lessons (2026-11-02-ai); vocab (my-words-ja); workspace (workspace)");
    }

    #[test]
    fn detects_keys_but_not_prose() {
        assert!(secret_like("key = sk-ant-api03-abcdefghijklmnopqrstuvwxyz").is_some());
        assert!(secret_like("ghp_0123456789abcdefghijklmnopqrstuvwxyz").is_some());
        assert!(secret_like("We ask the model (sk-ant-) nothing.").is_none());
    }
}
