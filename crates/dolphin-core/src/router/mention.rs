//! `@Name` mentions. A mention skips the moderator and goes straight to that agent.

use regex::Regex;
use std::sync::OnceLock;

use crate::model::Agent;

fn re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?:^|[^\w@])@([\p{L}\p{N}_\-]+)").unwrap())
}

/// Agent ids mentioned in `text`, in order of appearance, among `agents`.
pub fn parse(text: &str, agents: &[Agent]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for cap in re().captures_iter(text) {
        let name = cap[1].to_lowercase();
        if let Some(a) = agents.iter().find(|a| a.name.to_lowercase() == name || a.slug == name)
            && !out.contains(&a.id)
        {
            out.push(a.id.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AgentColor, AgentRole};

    fn agent(id: &str, name: &str) -> Agent {
        Agent {
            id: id.into(),
            slug: name.to_lowercase(),
            name: name.into(),
            role: AgentRole::Specialist,
            specialty: "coder".into(),
            job: String::new(),
            color: AgentColor::solid("#fff"),
            model: None,
            image_model: None,
            ios_model: None,
            persona: None,
            learning: true,
            suggest_persona: false,
            sort: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn finds_mentions_in_order_without_emails() {
        let agents = vec![agent("1", "Rick"), agent("2", "Prof")];
        assert_eq!(parse("@prof and @Rick, then @rick again", &agents), vec!["2", "1"]);
        assert!(parse("mail me at a@rick.com", &agents).is_empty());
        assert!(parse("@nobody", &agents).is_empty());
    }
}
