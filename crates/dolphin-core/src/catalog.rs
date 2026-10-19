//! Reasoning-model detection. Provider metadata wins where it exists (OpenRouter supported
//! parameters, Ollama capabilities, Anthropic/Gemini model capabilities); otherwise the bundled
//! catalog in `packages/shared/catalog/models.json` decides. Only reasoning models are selectable.

use std::sync::OnceLock;

use regex::Regex;
use serde::Deserialize;

use crate::model::{EffortControl, ModelInfo};

const CATALOG_JSON: &str = include_str!("../../../packages/shared/catalog/models.json");

#[derive(Deserialize)]
struct RawCatalog {
    rules: Vec<RawRule>,
    #[serde(rename = "imageModels")]
    image_models: Vec<RawImage>,
}

#[derive(Deserialize)]
struct RawRule {
    #[serde(rename = "match")]
    pattern: String,
    reasoning: bool,
    #[serde(default)]
    effort: Option<EffortControl>,
    #[serde(default)]
    vision: Option<bool>,
    #[serde(default)]
    tools: Option<bool>,
}

#[derive(Deserialize)]
struct RawImage {
    #[serde(rename = "match")]
    pattern: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CatalogHit {
    pub reasoning: bool,
    pub effort: EffortControl,
    pub vision: Option<bool>,
    pub tools: Option<bool>,
}

struct Catalog {
    rules: Vec<(Regex, CatalogHit)>,
    image: Vec<Regex>,
}

fn catalog() -> &'static Catalog {
    static C: OnceLock<Catalog> = OnceLock::new();
    C.get_or_init(|| {
        let raw: RawCatalog = serde_json::from_str(CATALOG_JSON).expect("bundled catalog is valid JSON");
        let re = |p: &str| Regex::new(&format!("(?i){p}")).expect("bundled catalog regex");
        Catalog {
            rules: raw
                .rules
                .into_iter()
                .map(|r| {
                    (
                        re(&r.pattern),
                        CatalogHit {
                            reasoning: r.reasoning,
                            effort: r.effort.unwrap_or(EffortControl::None),
                            vision: r.vision,
                            tools: r.tools,
                        },
                    )
                })
                .collect(),
            image: raw.image_models.into_iter().map(|r| re(&r.pattern)).collect(),
        }
    })
}

pub fn lookup(model_id: &str) -> Option<CatalogHit> {
    catalog().rules.iter().find(|(re, _)| re.is_match(model_id)).map(|(_, h)| h.clone())
}

pub fn looks_like_image_model(model_id: &str) -> bool {
    catalog().image.iter().any(|re| re.is_match(model_id))
}

pub const NOT_REASONING: &str = "Not a reasoning model. Dolphin Pod agents need models that think before answering.";
pub const UNKNOWN_MODEL: &str = "Not recognized as a reasoning model.";

/// Build a `ModelInfo` from the catalog alone (providers that only return ids).
pub fn info_from_catalog(id: &str, name: Option<&str>) -> ModelInfo {
    let hit = lookup(id);
    let reasoning = hit.as_ref().is_some_and(|h| h.reasoning);
    ModelInfo {
        id: id.to_string(),
        name: name.unwrap_or(id).to_string(),
        reasoning,
        reason: match &hit {
            Some(h) if h.reasoning => None,
            Some(_) => Some(NOT_REASONING.into()),
            None => Some(UNKNOWN_MODEL.into()),
        },
        effort: hit.as_ref().map(|h| h.effort.clone()).unwrap_or(EffortControl::None),
        vision: hit.as_ref().and_then(|h| h.vision).unwrap_or(false),
        tools: hit.as_ref().and_then(|h| h.tools).unwrap_or(true),
        image_output: looks_like_image_model(id) && !reasoning,
        context_window: None,
        free: id.ends_with(":free"),
    }
}

/// Sort: reasoning models first, then by name.
pub fn sort_models(models: &mut [ModelInfo]) {
    models.sort_by(|a, b| b.reasoning.cmp(&a.reasoning).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_reasoning_models() {
        for id in ["gpt-oss:20b", "o3-mini", "gpt-5.2", "deepseek-r1:14b", "qwen3:8b", "claude-opus-4-6",
            "claude-sonnet-4-5", "gemini-2.5-flash", "magistral-medium-latest", "openai/gpt-oss-120b",
            "grok-4-fast-reasoning", "qwq:32b", "phi4-reasoning:14b"] {
            assert!(lookup(id).is_some_and(|h| h.reasoning), "{id} should be reasoning");
        }
        for id in ["llama3.1:8b", "gemma3:12b", "gpt-4o", "mistral-small"] {
            assert!(!lookup(id).is_some_and(|h| h.reasoning), "{id} should not be reasoning");
        }
        assert!(!lookup("qwen3-coder:30b").unwrap().reasoning);
        assert!(!lookup("grok-4-fast-non-reasoning").unwrap().reasoning);
    }

    #[test]
    fn effort_controls_match_family() {
        assert!(matches!(lookup("gpt-oss:20b").unwrap().effort, EffortControl::Levels { .. }));
        assert!(matches!(lookup("claude-sonnet-4-5").unwrap().effort, EffortControl::Budget { .. }));
        assert!(matches!(lookup("claude-opus-5-5").unwrap().effort, EffortControl::Levels { .. }));
        assert!(matches!(lookup("qwen3:8b").unwrap().effort, EffortControl::Toggle));
        assert!(matches!(lookup("deepseek-reasoner").unwrap().effort, EffortControl::None));
    }

    #[test]
    fn unknown_models_are_disabled_with_reason() {
        let m = info_from_catalog("my-finetune", None);
        assert!(!m.reasoning);
        assert_eq!(m.reason.as_deref(), Some(UNKNOWN_MODEL));
        assert!(info_from_catalog("gpt-image-1", None).image_output);
    }
}
