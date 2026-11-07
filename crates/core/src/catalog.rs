//! Local model catalog (`models/catalog.json`) and the memory budget check.
//!
//! Brains run from the shared model folder ([`crate::local_llm`]); ears and
//! mouth are macOS built-in and take no model memory (docs/decisions.md
//! §14–15). The GPU share below is the old estimate; an M6 32 GB reports
//! 26.8 GB (`recommendedMaxWorkingSetSize`), so the check is conservative.

use serde::{Deserialize, Serialize};

const BUNDLED: &str = include_str!("../../../models/catalog.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub version: u32,
    #[serde(default)]
    pub note: String,
    pub budget_gb: f64,
    pub defaults: Defaults,
    pub cloud: Cloud,
    pub models: Vec<Model>,
}

/// Cloud models offered in settings; the first-run default comes from here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cloud {
    pub provider: String,
    pub default: String,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub note: String,
    pub models: Vec<CloudModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudModel {
    pub id: String,
    #[serde(default)]
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Defaults {
    pub brain: String,
    pub ears: String,
    pub mouth: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub role: String,
    pub name: String,
    pub variant: String,
    #[serde(default)]
    pub summary: String,
    pub repo: String,
    pub file_suffix: String,
    #[serde(default)]
    pub mmproj_suffix: Option<String>,
    pub approx_gb: f64,
    pub min_memory_gb: f64,
    #[serde(default)]
    pub context: Option<u32>,
    pub runtime: String,
    #[serde(default)]
    pub license: String,
}

impl Catalog {
    pub fn bundled() -> Catalog {
        serde_json::from_str(BUNDLED).expect("models/catalog.json is valid")
    }

    /// A user's own catalog replaces the bundled one when it parses.
    pub fn load(override_path: Option<&std::path::Path>) -> Catalog {
        override_path
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(Catalog::bundled)
    }

    pub fn get(&self, id: &str) -> Option<&Model> {
        self.models.iter().find(|m| m.id == id)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub models: Vec<String>,
    pub total_gb: f64,
    pub budget_gb: f64,
    /// About what macOS lets the GPU use by default (~2/3 of unified memory
    /// up to 36 GB, ~3/4 above).
    pub gpu_usable_gb: f64,
    pub fits: bool,
}

pub fn gpu_usable(memory_gb: f64) -> f64 {
    if memory_gb > 36.0 { memory_gb * 0.75 } else { memory_gb * 2.0 / 3.0 }
}

/// Brain + ears + mouth against the budget and the GPU share of memory.
pub fn plan(cat: &Catalog, ids: &[&str], memory_gb: f64) -> Plan {
    let total: f64 = ids.iter().filter_map(|id| cat.get(id)).map(|m| m.approx_gb).sum();
    let usable = gpu_usable(memory_gb);
    Plan {
        models: ids.iter().map(|s| s.to_string()).collect(),
        total_gb: (total * 100.0).round() / 100.0,
        budget_gb: cat.budget_gb,
        gpu_usable_gb: (usable * 10.0).round() / 10.0,
        fits: total <= cat.budget_gb && total <= usable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_fit_a_32gb_mac() {
        let c = Catalog::bundled();
        for id in [&c.defaults.brain, &c.defaults.ears, &c.defaults.mouth] {
            assert!(c.get(id).is_some(), "{id} in catalog");
        }
        let p = plan(&c, &[&c.defaults.brain, &c.defaults.ears, &c.defaults.mouth], 32.0);
        assert!(p.fits, "{p:?}");
        assert!((p.gpu_usable_gb - 21.3).abs() < 0.1);
        // Q8 brain + built-in voice within 20 GB on 32 GB; Qwen is offered.
        assert!(plan(&c, &["gemma-4-12b-q8", "apple-speech", "macos-voices"], 32.0).fits);
        assert!(!plan(&c, &["gemma-4-12b-q8"], 16.0).fits);
        assert!(c.models.iter().any(|m| m.role == "brain" && m.name.starts_with("Qwen")));
        assert!(c.models.iter().filter(|m| m.role == "brain").all(|m| m.runtime == "llama-server" && !m.repo.is_empty()));
        assert!(c.cloud.models.iter().any(|m| m.id == c.cloud.default));
    }
}
