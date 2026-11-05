//! Model-specific choices. Session-v1 stores omission as "off"; Rooxcode presents
//! that as Default, distinct from the explicit advertised effort "none".
use super::config::{Api, Authentication, Model, Provider};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl ReasoningEffort {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
        }
    }
}

pub(super) enum Policy<'a> {
    Rooxcode(&'a [ReasoningEffort]),
    Responses,
    Disabled,
}

impl<'a> Policy<'a> {
    pub fn for_model(provider: &Provider, model: &'a Model) -> Self {
        if provider.authentication == Authentication::Rooxcode {
            Self::Rooxcode(if model.reasoning {
                &model.reasoning_efforts
            } else {
                &[]
            })
        } else if model.reasoning && provider.api == Api::Responses {
            Self::Responses
        } else {
            Self::Disabled
        }
    }

    pub fn levels(&self) -> Vec<&'static str> {
        match self {
            Self::Rooxcode(efforts) => std::iter::once("default")
                .chain(efforts.iter().map(|effort| effort.as_str()))
                .collect(),
            Self::Responses => vec!["off", "low", "medium", "high"],
            Self::Disabled => vec!["off"],
        }
    }

    pub fn supports_stored(&self, level: &str) -> bool {
        level == "off"
            || match self {
                Self::Rooxcode(efforts) => efforts.iter().any(|effort| effort.as_str() == level),
                Self::Responses => matches!(level, "low" | "medium" | "high"),
                Self::Disabled => false,
            }
    }

    pub fn requested(&self, level: &str) -> Option<String> {
        self.levels()
            .contains(&level)
            .then(|| if level == "default" { "off" } else { level }.to_owned())
    }

    pub fn rooxcode_effort(&self, stored: &str) -> Option<ReasoningEffort> {
        match self {
            Self::Rooxcode(efforts) => efforts.iter().copied().find(|e| e.as_str() == stored),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_and_none_remain_distinct_and_existing_providers_keep_their_levels() {
        let policy = Policy::Rooxcode(&[ReasoningEffort::None, ReasoningEffort::Max]);
        assert_eq!(policy.levels(), ["default", "none", "max"]);
        assert_eq!(policy.requested("default").as_deref(), Some("off"));
        assert_eq!(policy.requested("none").as_deref(), Some("none"));
        assert_eq!(policy.rooxcode_effort("off"), None);
        assert_eq!(policy.rooxcode_effort("none"), Some(ReasoningEffort::None));
        assert!(!policy.supports_stored("high"));
        assert_eq!(Policy::Rooxcode(&[]).levels(), ["default"]);
        assert_eq!(Policy::Responses.levels(), ["off", "low", "medium", "high"]);
        assert!(Policy::Responses.supports_stored("high"));
        assert!(!Policy::Responses.supports_stored("none"));
        assert_eq!(Policy::Disabled.levels(), ["off"]);
        assert!(!Policy::Disabled.supports_stored("high"));
        for (wire, effort) in [
            ("none", ReasoningEffort::None),
            ("minimal", ReasoningEffort::Minimal),
            ("low", ReasoningEffort::Low),
            ("medium", ReasoningEffort::Medium),
            ("high", ReasoningEffort::High),
            ("xhigh", ReasoningEffort::Xhigh),
            ("max", ReasoningEffort::Max),
        ] {
            assert_eq!(serde_json::to_value(effort).unwrap(), wire);
            assert_eq!(effort.as_str(), wire);
        }
    }
}
