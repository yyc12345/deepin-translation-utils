// SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
//
// SPDX-License-Identifier: MIT

// transifex.yaml file spec: https://help.transifex.com/en/articles/6265125-github-installation-and-configuration#h_94380d9cd8

use std::{fs, path::PathBuf};

use regex::Regex;
use serde::{Serialize, Deserialize};
use thiserror::Error as TeError;

use super::tx_config_file::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct TransifexYaml {
    pub filters: Vec<Filter>,
    pub settings: Settings,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TxResourceLookupEntry {
    pub repository: String,
    /// Git branch name, not transifex branch name
    pub branch: String,
    pub resource: String,
    /// Full slug, i.e. `o:org:p:proj:r:res`
    pub transifex_resource_id: String,
}

impl TransifexYaml {
    pub fn to_tx_config(&self, github_repository: String, lookup_table: Vec<TxResourceLookupEntry>) -> TxConfig {
        let mut resource_sections = Vec::<TxConfigSectionResource>::new();
        for filter in &self.filters {
            let mut resource_section = TxConfigSectionResource::default();
            resource_section.source_file = filter.source.clone();
            resource_section.source_lang = filter.source_lang.clone();
            resource_section.type_attr = filter.format.clone();
            resource_section.file_filter = filter.target_pattern.clone();

            // from lookup table, find if we have resource have the same repository and resource name
            if let Some(lookup_entry) = lookup_table.iter().find(|entry| {
                entry.repository == github_repository && entry.resource == filter.source
            }) {
                resource_section.resource_full_slug = lookup_entry.transifex_resource_id.clone();
            } else {
                resource_section.resource_full_slug = format!("o:{}:p:{}:r:{}", "unknown-org", "unknown-proj", "unknown-res")
            }
            
            resource_sections.push(resource_section);
        };
        TxConfig {
            main_section: TxConfigSectionMain {
                host: "https://www.transifex.com".to_string(),
                ..TxConfigSectionMain::default()
            },
            resource_sections,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Filter {
    #[serde(rename = "filter_type")]
    pub type_attr: String,
    #[serde(rename = "source_file")]
    pub source: String,
    #[serde(rename = "file_format")]
    pub format: String,
    #[serde(rename = "source_language")]
    pub source_lang: String,
    #[serde(rename = "translation_files_expression")]
    pub target_pattern: String,
}

impl Filter {
    pub fn match_target_files(&self, project_root: &PathBuf) -> Result<Vec<(String, PathBuf)>, std::io::Error> {
        let target_pattern_path = project_root.join(&self.target_pattern);
        let Some(target_filename_pattern) = target_pattern_path.file_name() else {
            return Err(std::io::Error::new(std::io::ErrorKind::Other, "File name not found"));
        };
        let Some(target_filename_pattern) = target_filename_pattern.to_str() else {
            return Err(std::io::Error::new(std::io::ErrorKind::Other, "File name not valid"));
        };
        let Some(target_filter_pattern) = create_filter_pattern(target_filename_pattern) else {
            return Err(std::io::Error::new(std::io::ErrorKind::Other, "Filter pattern not valid"));
        };
        let Some(target_parent) = target_pattern_path.parent() else {
            return Err(std::io::Error::new(std::io::ErrorKind::Other, "Parent dir not found"));
        };
        let target_files = target_parent.read_dir()?;
        let mut matched_files = Vec::<(String, PathBuf)>::new();
        for file in target_files {
            let file = file?;
            let file_name = file.file_name();
            let Some(file_name) = file_name.to_str() else {
                continue;
            };
            target_filter_pattern.captures(file_name).and_then(|captures| {
                captures.get(1).map(|lang_code| {
                    let lang_code = lang_code.as_str();
                    matched_files.push((lang_code.to_string(), file.path()));
                })
            });
        };
        Ok(matched_files)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Settings {
    #[serde(rename = "pr_branch_name")]
    pub branch_template: String,
}

#[derive(TeError, Debug)]
pub enum TxYamlLoadError {
    #[error("File not found")]
    FileNotFound,
    #[error("Can not read file")]
    ReadFile(#[from] std::io::Error),
    #[error("Fail to deserialize file: {0}")]
    Serde(#[from] serde_yml::Error),
    #[error("Fail to convert from .tx/config file: {0:?}")]
    ConvertError(#[from] TxConfigLoadError),
}

pub fn try_laod_transifex_yaml_file(project_root: &PathBuf) -> Result<(PathBuf, TransifexYaml), TxYamlLoadError> {
    // try find transifex.yaml in project_root/transifex.yaml and if not found, try project_root/.tx/transifex.yaml. If still not found, return error.
    let transifex_yaml_file = project_root.join("transifex.yaml");
    if transifex_yaml_file.is_file() {
        let tx_yaml = load_tx_yaml_file(&transifex_yaml_file)?;
        return Ok((transifex_yaml_file, tx_yaml));
    }
    let transifex_yaml_file = project_root.join(".tx").join("transifex.yaml");
    if transifex_yaml_file.is_file() {
        let tx_yaml = load_tx_yaml_file(&transifex_yaml_file)?;
        return Ok((transifex_yaml_file, tx_yaml));
    }

    Err(TxYamlLoadError::FileNotFound)
}

pub fn load_tx_yaml_file(transifex_yaml_file: &PathBuf) -> Result<TransifexYaml, TxYamlLoadError> {
    if !transifex_yaml_file.is_file() {
        return Err(TxYamlLoadError::FileNotFound);
    }
    let source_content = fs::read_to_string(&transifex_yaml_file)?;
    Ok(serde_yml::from_str::<TransifexYaml>(source_content.as_str())?)
}

fn create_filter_pattern(pattern: &str) -> Option<Regex> {
    let parts: Vec<&str> = pattern.split("<lang>").collect();
    if parts.len() != 2 {
        return None;
    }

    let regex_pattern = format!(
        r#"^{}([a-z_A-Z]{{2,6}}){}$"#,
        regex::escape(parts[0]),
        regex::escape(parts[1])
    );

    Regex::new(&regex_pattern).ok()
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub const TEST_TX_YAML_CONTENT: &str = r#"# Some comments or spdx license headers
filters:
  - filter_type: file
    source_file: shell-launcher-applet/translations/org.deepin.ds.dock.launcherapplet.ts
    file_format: QT
    source_language: en_US
    translation_files_expression: shell-launcher-applet/translations/org.deepin.ds.dock.launcherapplet_<lang>.ts
settings:
  pr_branch_name: transifex_update_<br_unique_id>
"#;

    #[test]
    fn tst_parse_tx_yaml_content() {
        let tx_yaml: TransifexYaml = serde_yml::from_str::<TransifexYaml>(TEST_TX_YAML_CONTENT).unwrap();
        assert_eq!(tx_yaml.filters.len(), 1);
        assert_eq!(tx_yaml.filters[0].type_attr, "file");
        assert_eq!(tx_yaml.filters[0].source, "shell-launcher-applet/translations/org.deepin.ds.dock.launcherapplet.ts");
        assert_eq!(tx_yaml.filters[0].format, "QT");
        assert_eq!(tx_yaml.filters[0].source_lang, "en_US");
        assert_eq!(tx_yaml.filters[0].target_pattern, "shell-launcher-applet/translations/org.deepin.ds.dock.launcherapplet_<lang>.ts");
    }

    #[test]
    fn test_pathbuf() {
        let path = PathBuf::from("/example/sample_<lang>.ts");
        assert_eq!(path.file_name(), Some(std::ffi::OsStr::new("sample_<lang>.ts")));
        let pattern = create_filter_pattern(path.to_str().unwrap()).unwrap();
        let matched = pattern.captures("/example/sample_zh_CN.ts").and_then(|caps| caps.get(1)).map(|m| {
            m.as_str().to_string()
        });
        assert_eq!(matched, Some("zh_CN".to_string()));
    }
}
