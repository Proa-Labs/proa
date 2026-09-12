// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Deterministic capability resolution for application-first site scaffolds.

use anyhow::{bail, Result};
use clap::ValueEnum;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum SiteTemplate {
    App,
    Marketing,
    Docs,
    Minimal,
    Origin,
}

impl SiteTemplate {
    pub fn parse_compat(value: &str) -> Result<Self> {
        match value {
            "app" | "basic" => Ok(Self::App),
            "marketing" => Ok(Self::Marketing),
            "docs" => Ok(Self::Docs),
            "minimal" => Ok(Self::Minimal),
            "origin" => Ok(Self::Origin),
            other => {
                bail!("unknown template `{other}` (expected app, marketing, docs, minimal, or origin)")
            }
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Marketing => "marketing",
            Self::Docs => "docs",
            Self::Minimal => "minimal",
            Self::Origin => "origin",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseChoice {
    Sqlite,
    None,
}

impl DatabaseChoice {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum UiChoice {
    Proa,
    None,
    #[value(skip)]
    ProaDocs,
}

impl UiChoice {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Proa => "proa",
            Self::None => "none",
            Self::ProaDocs => "proa-docs",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum StylingChoice {
    Tailwind,
    Css,
    #[value(skip)]
    Docs,
}

impl StylingChoice {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tailwind => "tailwind",
            Self::Css => "css",
            Self::Docs => "docs",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum StarterChoice {
    Guided,
    Blank,
}

impl StarterChoice {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Guided => "guided",
            Self::Blank => "blank",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum DeployChoice {
    None,
    Docker,
    Railway,
    Coolify,
}

impl DeployChoice {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Docker => "docker",
            Self::Railway => "railway",
            Self::Coolify => "coolify",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitySelection {
    pub template: SiteTemplate,
    pub database: DatabaseChoice,
    pub ui: UiChoice,
    pub styling: StylingChoice,
    pub starter: StarterChoice,
    pub deploy: DeployChoice,
    pub markdown: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CapabilityOverrides {
    pub database: Option<DatabaseChoice>,
    pub ui: Option<UiChoice>,
    pub styling: Option<StylingChoice>,
    pub starter: Option<StarterChoice>,
    pub deploy: Option<DeployChoice>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityId {
    pub id: &'static str,
    pub plan_version: u32,
}

impl CapabilitySelection {
    pub fn resolve(template: SiteTemplate, overrides: CapabilityOverrides) -> Result<Self> {
        let defaults = match template {
            SiteTemplate::App => Self {
                template,
                database: DatabaseChoice::Sqlite,
                ui: UiChoice::Proa,
                styling: StylingChoice::Tailwind,
                starter: StarterChoice::Guided,
                deploy: DeployChoice::None,
                markdown: true,
            },
            SiteTemplate::Marketing => Self {
                template,
                database: DatabaseChoice::None,
                ui: UiChoice::Proa,
                styling: StylingChoice::Tailwind,
                starter: StarterChoice::Guided,
                deploy: DeployChoice::None,
                markdown: true,
            },
            SiteTemplate::Docs => Self {
                template,
                database: DatabaseChoice::None,
                ui: UiChoice::ProaDocs,
                styling: StylingChoice::Docs,
                starter: StarterChoice::Guided,
                deploy: DeployChoice::None,
                markdown: true,
            },
            SiteTemplate::Minimal => Self {
                template,
                database: DatabaseChoice::None,
                ui: UiChoice::None,
                styling: StylingChoice::Css,
                starter: StarterChoice::Blank,
                deploy: DeployChoice::None,
                markdown: false,
            },
            SiteTemplate::Origin => Self {
                template,
                database: DatabaseChoice::None,
                ui: UiChoice::None,
                styling: StylingChoice::Tailwind,
                starter: StarterChoice::Guided,
                deploy: DeployChoice::None,
                markdown: true,
            },
        };

        if template == SiteTemplate::Origin
            && (overrides.database.is_some()
                || overrides.ui.is_some()
                || overrides.styling.is_some()
                || overrides.starter.is_some())
        {
            bail!(
                "the origin preset owns its application content; only --deploy can be overridden"
            );
        }

        let selection = Self {
            database: overrides.database.unwrap_or(defaults.database),
            ui: overrides.ui.unwrap_or(defaults.ui),
            styling: overrides.styling.unwrap_or(defaults.styling),
            starter: overrides.starter.unwrap_or(defaults.starter),
            deploy: overrides.deploy.unwrap_or(defaults.deploy),
            ..defaults
        };
        if selection.template == SiteTemplate::Docs && selection.ui == UiChoice::Proa {
            bail!("the docs preset owns its proa-docs UI; --ui proa is not supported yet");
        }
        Ok(selection)
    }

    pub fn capabilities(self) -> Vec<CapabilityId> {
        let mut values = vec![
            CapabilityId {
                id: "base:site",
                plan_version: 1,
            },
            CapabilityId {
                id: "server:axum",
                plan_version: 1,
            },
        ];
        if self.database == DatabaseChoice::Sqlite {
            values.push(CapabilityId {
                id: "database:sqlite",
                plan_version: 1,
            });
        }
        if self.markdown {
            values.push(CapabilityId {
                id: "representation:markdown",
                plan_version: 1,
            });
        }
        if self.ui == UiChoice::Proa {
            values.push(CapabilityId {
                id: "ui:proa",
                plan_version: 1,
            });
        }
        if self.ui == UiChoice::ProaDocs {
            values.push(CapabilityId {
                id: "content:docs",
                plan_version: 1,
            });
        }
        values.push(CapabilityId {
            id: match self.styling {
                StylingChoice::Tailwind => "styling:tailwind",
                StylingChoice::Css => "styling:css",
                StylingChoice::Docs => "styling:docs",
            },
            plan_version: 1,
        });
        values.push(CapabilityId {
            id: match self.starter {
                StarterChoice::Guided => "starter:guided",
                StarterChoice::Blank => "starter:blank",
            },
            plan_version: 1,
        });
        if self.deploy != DeployChoice::None {
            values.push(CapabilityId {
                id: "deploy:docker",
                plan_version: 1,
            });
            if self.deploy == DeployChoice::Railway {
                values.push(CapabilityId {
                    id: "deploy:railway",
                    plan_version: 1,
                });
            }
            if self.deploy == DeployChoice::Coolify {
                values.push(CapabilityId {
                    id: "deploy:coolify",
                    plan_version: 1,
                });
            }
        }
        values
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_defaults_match_the_application_first_contract() {
        let selected =
            CapabilitySelection::resolve(SiteTemplate::App, CapabilityOverrides::default())
                .unwrap();
        assert_eq!(selected.database, DatabaseChoice::Sqlite);
        assert_eq!(selected.ui, UiChoice::Proa);
        assert_eq!(selected.styling, StylingChoice::Tailwind);
        assert_eq!(selected.starter, StarterChoice::Guided);
        assert!(selected.markdown);
    }

    #[test]
    fn basic_is_the_only_template_alias() {
        assert_eq!(
            SiteTemplate::parse_compat("basic").unwrap(),
            SiteTemplate::App
        );
        assert!(SiteTemplate::parse_compat("dashboard").is_err());
    }
}
