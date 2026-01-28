use std::collections::{HashMap, HashSet};
use std::io::Write;

use anyhow::Result;
use ini::Ini;
use reqwest::blocking::Client;
use reqwest::header::USER_AGENT;
use serde::Deserialize;

use crate::{buckal_log, buckal_warn, user_agent};

type Section = String;
type Lines = Vec<String>;

// TODO: too complicated, try to simplify this
struct BuckConfig {
    section_order: Vec<Section>,
    raw_sections: HashMap<Section, Lines>,
    raw_section_names: HashSet<Section>,
    touched_sections: HashSet<Section>,
    ini: Ini,
}

impl Default for BuckConfig {
    fn default() -> Self {
        Self {
            section_order: Vec::new(),
            raw_sections: HashMap::new(),
            raw_section_names: HashSet::new(),
            touched_sections: HashSet::new(),
            ini: Ini::new(),
        }
    }
}

impl BuckConfig {
    pub fn load(path: &std::path::Path) -> std::io::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        Ok(Self::parse(contents))
    }

    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        std::fs::write(path, self.serialize())?;
        Ok(())
    }

    pub fn upsert_kv(&mut self, section: &str, key: &str, value: &str) {
        self.ensure_section(section);
        self.touched_sections.insert(section.to_string());
        self.raw_section_names.remove(section);
        self.ini
            .with_section(Some(section.to_string()))
            .set(key.to_string(), value.to_string());
    }

    pub fn clear_section(&mut self, section: &str) {
        self.touched_sections.insert(section.to_string());
        self.raw_section_names.remove(section);
        self.raw_sections.insert(section.to_string(), Vec::new());
        self.ini.delete(Some(section.to_string()));
    }

    pub fn ensure_section(&mut self, section: &str) {
        if !self.section_order.iter().any(|s| s == section) {
            self.section_order.push(section.to_string());
        }
        self.raw_sections.entry(section.to_string()).or_default();
    }

    pub fn ensure_section_after(&mut self, after_section: &str, section: &str) {
        if self.section_order.iter().any(|s| s == section) {
            return;
        }
        if let Some(pos) = self.section_order.iter().position(|s| s == after_section) {
            self.section_order.insert(pos + 1, section.to_string());
        } else {
            self.section_order.push(section.to_string());
        }
        self.raw_sections.entry(section.to_string()).or_default();
    }

    /// Append a key-value pair at the end of a section's raw lines (preserves insertion order).
    /// This keeps the section in "raw" mode (not touched by ini).
    pub fn append_kv(&mut self, section: &str, key: &str, value: &str) {
        self.ensure_section(section);
        let line = format!("  {} = {}", key, value);
        self.raw_sections
            .entry(section.to_string())
            .or_default()
            .push(line);
        // Also update ini for consistency
        self.ini
            .with_section(Some(section.to_string()))
            .set(key.to_string(), value.to_string());
    }

    /// Insert a comment line before a specific key in a section.
    /// The comment should not include the leading `# ` - it will be added automatically.
    /// The comment will use the same indentation as the key line.
    pub fn insert_comment_before_key(&mut self, section: &str, key: &str, comment: &str) {
        if let Some(lines) = self.raw_sections.get_mut(section) {
            let key_pattern = format!("{} = ", key);
            if let Some(pos) = lines
                .iter()
                .position(|line| line.trim_start().starts_with(&key_pattern))
            {
                let indent = lines[pos].len() - lines[pos].trim_start().len();
                let indent_str = &lines[pos][..indent];
                lines.insert(pos, format!("{}# {}", indent_str, comment));
            }
        }
    }

    fn parse(contents: String) -> BuckConfig {
        let ini = Ini::load_from_str(&contents).unwrap_or_else(|_| Ini::new());
        let mut config = BuckConfig {
            ini,
            ..Default::default()
        };
        let mut current_section: Option<String> = None;

        for line in contents.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                let section_name = trimmed[1..trimmed.len() - 1].to_string();
                config.section_order.push(section_name.clone());
                config.raw_sections.entry(section_name.clone()).or_default();
                current_section = Some(section_name);
            } else if let Some(section) = &current_section {
                if !trimmed.is_empty()
                    && !trimmed.starts_with('#')
                    && !trimmed.starts_with(';')
                    && !trimmed.contains('=')
                {
                    config.raw_section_names.insert(section.clone());
                }
                config
                    .raw_sections
                    .entry(section.clone())
                    .or_default()
                    .push(line.to_string());
            }
        }
        config
    }

    fn serialize(&self) -> String {
        let mut output = String::new();

        for section in &self.section_order {
            output.push('[');
            output.push_str(section);
            output.push_str("]\n");
            let ini_section = self.ini.section(Some(section.as_str()));
            let use_raw = !self.touched_sections.contains(section);
            if use_raw {
                if let Some(lines) = self.raw_sections.get(section) {
                    for line in lines {
                        output.push_str(line);
                        output.push('\n');
                    }
                    let last_non_empty = lines.iter().rev().find(|line| !line.trim().is_empty());
                    let ends_with_comment = last_non_empty.is_some_and(|line| {
                        let trimmed = line.trim();
                        trimmed.starts_with('#') || trimmed.starts_with(';')
                    });
                    let last_blank = lines.last().is_some_and(|line| line.trim().is_empty());
                    if !last_blank && !ends_with_comment {
                        output.push('\n');
                    }
                }
            } else if let Some(lines) = ini_section {
                let mut items: Vec<(String, String)> = lines
                    .iter()
                    .map(|(key, value)| (key.to_string(), value.to_string()))
                    .collect();
                items.sort_by(|(left, _), (right, _)| left.cmp(right));
                for (key, value) in items {
                    output.push_str("  ");
                    output.push_str(&key);
                    output.push_str(" = ");
                    output.push_str(&value);
                    output.push('\n');
                }
                output.push('\n');
            }
        }
        while output.ends_with('\n') {
            output.pop();
        }

        output
    }
}

pub fn init_modifier(dest: &std::path::Path) -> Result<()> {
    let mut package_file = std::fs::File::create(dest.join("PACKAGE"))?;

    writeln!(package_file, "# @generated by `cargo buckal`")?;
    writeln!(package_file)?;
    writeln!(
        package_file,
        "load(\"@prelude//cfg/modifier:set_cfg_modifiers.bzl\", \"set_cfg_modifiers\")"
    )?;
    writeln!(
        package_file,
        "load(\"@prelude//rust:with_workspace.bzl\", \"with_rust_workspace\")"
    )?;
    writeln!(
        package_file,
        "load(\"@buckal//config:set_cfg_constructor.bzl\", \"set_cfg_constructor\")"
    )?;
    writeln!(package_file)?;
    writeln!(package_file, "ALIASES = {{")?;
    writeln!(
        package_file,
        "    \"debug\": \"buckal//config/mode:debug\","
    )?;
    writeln!(
        package_file,
        "    \"release\": \"buckal//config/mode:release\","
    )?;
    writeln!(package_file, "}}")?;
    writeln!(package_file, "set_cfg_constructor(aliases = ALIASES)")?;
    writeln!(package_file)?;
    writeln!(package_file, "set_cfg_modifiers(")?;
    writeln!(package_file, "    cfg_modifiers = [")?;
    writeln!(package_file, "        \"buckal//config/mode:debug\",")?;
    writeln!(package_file, "    ],")?;
    writeln!(package_file, ")")?;

    Ok(())
}

pub fn init_buckal_cell(dest: &std::path::Path) -> Result<()> {
    let mut buckconfig = BuckConfig::load(&dest.join(".buckconfig"))?;
    buckconfig.upsert_kv("cells", "buckal", "buckal");
    buckconfig.append_kv("external_cells", "buckal", "git");
    buckconfig.insert_comment_before_key(
        "external_cells",
        "buckal",
        "Added by cargo-buckal. See [external_cell_buckal] for git configuration.",
    );
    buckconfig.ensure_section_after("external_cells", "external_cell_buckal");
    buckconfig.clear_section("external_cell_buckal");
    buckconfig.upsert_kv(
        "external_cell_buckal",
        "git_origin",
        &format!("https://github.com/{}", crate::BUCKAL_BUNDLES_REPO),
    );
    let commit_hash = match fetch() {
        Ok(hash) => hash,
        Err(e) => {
            buckal_warn!(
                "Failed to fetch latest bundle hash ({}), using default hash instead.",
                e
            );
            crate::DEFAULT_BUNDLE_HASH.to_string()
        }
    };
    buckconfig.upsert_kv("external_cell_buckal", "commit_hash", &commit_hash);
    buckconfig.ensure_section("project");
    buckconfig.clear_section("project");
    buckconfig.upsert_kv("project", "ignore", ".git .buckal buck-out target");
    buckconfig.save(&dest.join(".buckconfig"))?;

    Ok(())
}

pub fn fetch_buckal_cell(dest: &std::path::Path) -> Result<()> {
    let mut buckconfig = BuckConfig::load(&dest.join(".buckconfig"))?;
    buckconfig.ensure_section("external_cell_buckal");
    buckconfig.clear_section("external_cell_buckal");
    buckconfig.upsert_kv(
        "external_cell_buckal",
        "git_origin",
        &format!("https://github.com/{}", crate::BUCKAL_BUNDLES_REPO),
    );
    let commit_hash = match fetch() {
        Ok(hash) => hash,
        Err(e) => {
            buckal_warn!(
                "Failed to fetch latest bundle hash ({}), using default hash instead.",
                e
            );
            crate::DEFAULT_BUNDLE_HASH.to_string()
        }
    };
    buckconfig.upsert_kv("external_cell_buckal", "commit_hash", &commit_hash);
    buckconfig.save(&dest.join(".buckconfig"))?;

    Ok(())
}

#[derive(Deserialize)]
struct GithubCommit {
    sha: String,
}

pub fn fetch() -> Result<String> {
    let url = format!(
        "https://api.github.com/repos/{}/commits",
        crate::BUCKAL_BUNDLES_REPO
    );
    buckal_log!(
        "Fetching",
        format!("https://github.com/{}", crate::BUCKAL_BUNDLES_REPO)
    );
    let client = Client::new();
    let response: Vec<GithubCommit> = client
        .get(&url)
        .header(USER_AGENT, user_agent())
        .query(&[("per_page", "1")])
        .send()?
        .json()?;
    Ok(response[0].sha.clone())
}

#[cfg(test)]
mod tests {
    use super::BuckConfig;
    use indoc::indoc;

    #[test]
    fn serialize_preserves_raw_sections_when_untouched() {
        let contents = indoc! {r#"
            [cells]
              root = .
              prelude = prelude

            [parser]
              target_platform_detector_spec = target:root//...->prelude//platforms:default \
                target:prelude//...->prelude//platforms:default

            [external_cells]
              prelude = bundled
        "#};
        let config = BuckConfig::parse(contents.trim_end().to_string());
        let output = config.serialize();
        assert_eq!(output, contents.trim_end());
    }

    #[test]
    fn serialize_uses_ini_for_touched_sections() {
        let contents = indoc! {r#"
            [cells]
              root = .
              prelude = prelude

            [parser]
              target_platform_detector_spec = target:root//...->prelude//platforms:default \
                target:prelude//...->prelude//platforms:default

            [external_cell_buckal]
              git_origin = old
              commit_hash = oldhash
        "#};
        let mut config = BuckConfig::parse(contents.trim_end().to_string());

        config.upsert_kv("cells", "buckal", "buckal");
        config.clear_section("external_cell_buckal");
        config.upsert_kv(
            "external_cell_buckal",
            "git_origin",
            "https://example.com/repo",
        );
        config.upsert_kv("external_cell_buckal", "commit_hash", "deadbeef");

        let output = config.serialize();
        let expected = indoc! {r#"
            [cells]
              buckal = buckal
              prelude = prelude
              root = .

            [parser]
              target_platform_detector_spec = target:root//...->prelude//platforms:default \
                target:prelude//...->prelude//platforms:default

            [external_cell_buckal]
              commit_hash = deadbeef
              git_origin = https://example.com/repo
        "#};
        assert_eq!(output, expected.trim_end());
    }

    #[test]
    fn serialize_no_extra_blank_line_after_comment() {
        let contents = indoc! {r#"
            [cells]
              root = .
              prelude = prelude
              toolchains = toolchains
              none = none

            [cell_aliases]
              config = prelude
              ovr_config = prelude
              fbcode = none
              fbsource = none
              fbcode_macros = none
              buck = none

            # Uses a copy of the prelude bundled with the buck2 binary. You can alternatively delete this
            # section and vendor a copy of the prelude to the `prelude` directory of your project.
            [external_cells]
              prelude = bundled

            [parser]
              target_platform_detector_spec = target:root//...->prelude//platforms:default \
                target:prelude//...->prelude//platforms:default \
                target:toolchains//...->prelude//platforms:default

            [build]
              execution_platforms = prelude//platforms:default
        "#};
        let config = BuckConfig::parse(contents.trim_end().to_string());
        let output = config.serialize();
        assert_eq!(output, contents.trim_end());
    }

    #[test]
    fn serialize_preserves_blank_lines_between_comment_and_code() {
        let contents = indoc! {r#"
            [section_a]
              key1 = value1
              key2 = value2

            # This comment has a blank line before the next section

            [section_b]
              key3 = value3
        "#};
        let config = BuckConfig::parse(contents.trim_end().to_string());
        let output = config.serialize();
        assert_eq!(output, contents.trim_end());
    }

    #[test]
    fn append_kv_and_comment() {
        let contents = indoc! {r#"
            [external_cells]
              prelude = bundled
        "#};
        let mut config = BuckConfig::parse(contents.trim_end().to_string());
        config.append_kv("external_cells", "buckal", "git");
        config.insert_comment_before_key(
            "external_cells",
            "buckal",
            "buckal cell for cargo-buckal",
        );
        let output = config.serialize();
        let expected = indoc! {r#"
            [external_cells]
              prelude = bundled
              # buckal cell for cargo-buckal
              buckal = git
        "#};
        assert_eq!(output, expected.trim_end());
    }
}
