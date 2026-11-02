//! robots.txt: parse the groups and answer "may this agent fetch this path".
//!
//! Follows RFC 9309: the most specific matching user-agent group applies
//! (falling back to `*`); within it the longest matching rule wins and
//! `Allow` wins a tie. `*` and a trailing `$` are supported in paths.

#[derive(Debug, Clone, Default)]
pub struct Robots {
    groups: Vec<Group>,
    pub crawl_delay: Option<f64>,
}

#[derive(Debug, Clone, Default)]
struct Group {
    agents: Vec<String>,
    rules: Vec<(bool, String)>, // (allow, pattern)
    delay: Option<f64>,
}

impl Robots {
    /// Everything allowed (no robots.txt, or 4xx).
    pub fn allow_all() -> Robots {
        Robots::default()
    }

    /// Everything disallowed (robots.txt unreachable: be conservative).
    pub fn deny_all() -> Robots {
        Robots { groups: vec![Group { agents: vec!["*".into()], rules: vec![(false, "/".into())], delay: None }], crawl_delay: None }
    }

    pub fn parse(text: &str) -> Robots {
        let mut groups: Vec<Group> = Vec::new();
        let mut cur: Option<Group> = None;
        let mut last_was_agent = false;
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            let Some((key, value)) = line.split_once(':') else { continue };
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim();
            match key.as_str() {
                "user-agent" => {
                    if !last_was_agent {
                        if let Some(g) = cur.take() {
                            groups.push(g);
                        }
                        cur = Some(Group::default());
                    }
                    if let Some(g) = cur.as_mut() {
                        g.agents.push(value.to_ascii_lowercase());
                    }
                    last_was_agent = true;
                }
                "allow" | "disallow" => {
                    last_was_agent = false;
                    if let Some(g) = cur.as_mut() {
                        // An empty Disallow means "allow everything".
                        if !value.is_empty() {
                            g.rules.push((key == "allow", value.to_string()));
                        }
                    }
                }
                "crawl-delay" => {
                    last_was_agent = false;
                    if let Some(g) = cur.as_mut() {
                        g.delay = value.parse().ok();
                    }
                }
                _ => {
                    last_was_agent = false;
                }
            }
        }
        if let Some(g) = cur.take() {
            groups.push(g);
        }
        Robots { groups, crawl_delay: None }
    }

    fn group_for(&self, agent: &str) -> Option<&Group> {
        let agent = agent.to_ascii_lowercase();
        // Product token: "JELearnerBot/0.1 (+url)" -> "jelearnerbot".
        let token = agent.split(['/', ' ']).next().unwrap_or("");
        self.groups
            .iter()
            .filter(|g| g.agents.iter().any(|a| a != "*" && !a.is_empty() && token.starts_with(a.as_str())))
            .max_by_key(|g| g.agents.iter().map(|a| a.len()).max().unwrap_or(0))
            .or_else(|| self.groups.iter().find(|g| g.agents.iter().any(|a| a == "*")))
    }

    pub fn allowed(&self, agent: &str, path: &str) -> bool {
        let Some(g) = self.group_for(agent) else { return true };
        let mut best: Option<(usize, bool)> = None;
        for (allow, pat) in &g.rules {
            if matches(pat, path) {
                let len = pat.len();
                best = match best {
                    Some((l, a)) if l > len || (l == len && a) => Some((l, a)),
                    _ => Some((len, *allow)),
                };
            }
        }
        best.map(|(_, a)| a).unwrap_or(true)
    }

    pub fn delay_for(&self, agent: &str) -> Option<f64> {
        self.group_for(agent).and_then(|g| g.delay).or(self.crawl_delay)
    }
}

/// Path pattern match with `*` (any run) and trailing `$` (end anchor).
fn matches(pattern: &str, path: &str) -> bool {
    let full = match pattern.strip_suffix('$') {
        Some(p) => p.to_string(),
        None => format!("{pattern}*"),
    };
    glob(full.as_bytes(), path.as_bytes())
}

/// Whole-string match where `*` matches any run of bytes.
fn glob(p: &[u8], s: &[u8]) -> bool {
    let (mut pi, mut si) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while si < s.len() {
        if pi < p.len() && p[pi] == b'*' {
            star = Some(pi);
            mark = si;
            pi += 1;
        } else if pi < p.len() && p[pi] == s[si] {
            pi += 1;
            si += 1;
        } else if let Some(st) = star {
            pi = st + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == b'*')
}

#[cfg(test)]
mod tests {
    use super::*;

    const UA: &str = "JELearnerBot/0.1 (+https://github.com/ellrin/JELearner)";

    #[test]
    fn specific_group_and_longest_match() {
        let r = Robots::parse(
            "User-agent: *\nDisallow: /private\n\n\
             User-agent: JELearnerBot\nUser-agent: other\nDisallow: /news/\nAllow: /news/tech/\nCrawl-delay: 2\n",
        );
        assert!(!r.allowed(UA, "/news/world/1"));
        assert!(r.allowed(UA, "/news/tech/1"));
        assert!(r.allowed(UA, "/private"), "our group replaces the * group");
        assert!(!r.allowed("SomeBot", "/private/x"));
        assert_eq!(r.delay_for(UA), Some(2.0));
    }

    #[test]
    fn wildcards_and_anchor() {
        let r = Robots::parse("User-agent: *\nDisallow: /*.pdf$\nDisallow: /search*q=\nDisallow:\n");
        assert!(!r.allowed(UA, "/files/a.pdf"));
        assert!(r.allowed(UA, "/files/a.pdf?x=1"));
        assert!(!r.allowed(UA, "/search?lang=en&q=ai"));
        assert!(r.allowed(UA, "/"));
    }

    #[test]
    fn deny_all_blocks_everything() {
        assert!(!Robots::deny_all().allowed(UA, "/"));
        assert!(Robots::allow_all().allowed(UA, "/anything"));
    }
}
