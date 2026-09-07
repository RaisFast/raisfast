//! Skill loading & prompt rendering (M5-A; behavior ported from zeroclaw
//! `skills/mod.rs` skills_to_prompt_with_mode* and skill bundles).
//!
//! Layout (skills.md §2/§12-A): directories under a root
//!   platform/<name>/SKILL.md      (platform-level)
//!   tenants/<tenant>/<name>/SKILL.md
//!   users/<user>/<name>/SKILL.md  (reserved)
//! Enabling is agent-scoped: `ai_agents.params.skill_bundles` lists the skill
//! directory names (or `["*"]` for all in scope). Rendering mirrors zeroclaw:
//! Full inlines instructions; Compact emits metadata + a `read_skill` hint.

use std::fs;
use std::path::{Path, PathBuf};

use raisfast_agent::{SkillDocError, SkillDocument};

/// One enabled, parsed skill.
#[derive(Debug, Clone)]
pub struct LoadedSkill {
    pub name: String,
    pub description: String,
    /// Full SKILL.md body (instructions), used by Full mode / read_skill.
    pub instructions: String,
    pub always: bool,
    /// Declared platform tools this skill wants composed `skill__<tool>`
    /// wrappers for (from optional frontmatter `tools:`; §12-B).
    pub tools: Vec<String>,
    /// `tools` entries to exclude from the execution surface (availability
    /// removal; `allowed-tools` stays a no-op per §12-C).
    pub disallowed_tools: Vec<String>,
    /// Absolute path to the skill directory (for read_skill / audit).
    pub dir: PathBuf,
}

/// Root for skill directories. Override with `RAISFAST_SKILLS_DIR`.
pub fn skills_root() -> PathBuf {
    std::env::var("RAISFAST_SKILLS_DIR")
        .ok()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("storage/skills"))
}

/// Parse `ai_agents.params.skill_bundles` (JSON array of names, or `"*"`).
pub fn enabled_bundles(agent: &crate::agent::models::ai_agent::AiAgent) -> Vec<String> {
    agent
        .params
        .as_ref()
        .and_then(|p| p.get("skill_bundles"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Scan the enabled skills for a tenant (platform + tenant layers).
/// Malformed skills are skipped with a warn (behavior like zeroclaw audit).
pub fn load_skills(root: &Path, tenant: Option<&str>, enabled: &[String]) -> Vec<LoadedSkill> {
    if enabled.is_empty() {
        return Vec::new();
    }
    let all = enabled.iter().any(|e| e == "*");
    let mut layers: Vec<PathBuf> = vec![root.join("platform")];
    if let Some(t) = tenant {
        layers.push(root.join("tenants").join(t));
    }

    let mut out: Vec<LoadedSkill> = Vec::new();
    for layer in layers {
        let Ok(entries) = fs::read_dir(&layer) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !all && !enabled.iter().any(|e| e == &name) {
                continue;
            }
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            // Precedence: first layer seen wins (platform > user > tenant).
            if out.iter().any(|s| s.name == name) {
                continue;
            }
            match load_one(&dir) {
                Ok(mut skill) => {
                    skill.dir = dir;
                    out.push(skill);
                }
                Err(e) => {
                    tracing::warn!(skill = %name, error = %e, "skill load skipped");
                }
            }
        }
    }
    out
}

fn load_one(dir: &Path) -> Result<LoadedSkill, SkillDocError> {
    let raw =
        fs::read_to_string(dir.join("SKILL.md")).map_err(|e| SkillDocError::Io(e.to_string()))?;
    let doc = SkillDocument::parse(&raw)?;
    Ok(LoadedSkill {
        name: doc.frontmatter.name,
        description: doc.frontmatter.description,
        instructions: doc.body.trim().to_string(),
        always: doc.frontmatter.always,
        tools: doc.frontmatter.tools,
        disallowed_tools: doc.frontmatter.disallowed_tools,
        dir: dir.to_path_buf(),
    })
}

/// Render the `## Available Skills` section. Full inlines instructions;
/// Compact lists metadata + a read_skill hint. Text copied from zeroclaw
/// `skills_to_prompt_with_mode_and_availability` (verbatim preambles).
pub fn render_skills(skills: &[LoadedSkill], full: bool) -> Option<String> {
    if skills.is_empty() {
        return None;
    }
    let mut out = if full {
        String::from(
            "## Available Skills\n\n\
             Skill instructions and tool metadata are preloaded below.\n\
             Follow these instructions directly; do not read skill files at runtime unless the user asks.\n\n",
        )
    } else {
        String::from(
            "## Available Skills\n\n\
             Skill summaries are preloaded below to keep context compact.\n\
             Skill instructions are loaded on demand: call `read_skill(name)` with the skill's `<name>` when you need the full skill file.\n\
             Skills marked `always` include full instructions below even in compact mode.\n\n",
        )
    };
    for skill in skills {
        out.push_str("  <skill>\n");
        out.push_str(&format!("    <name>{}</name>\n", xml_escape(&skill.name)));
        out.push_str(&format!(
            "    <description>{}</description>\n",
            xml_escape(&skill.description)
        ));
        if (full || skill.always) && !skill.instructions.is_empty() {
            out.push_str("    <instructions>\n");
            for line in skill.instructions.lines() {
                out.push_str(&format!(
                    "      <instruction>{}</instruction>\n",
                    xml_escape(line)
                ));
            }
            out.push_str("    </instructions>\n");
        }
        out.push_str("  </skill>\n");
    }
    Some(out.trim_end().to_string())
}

/// Escape `<`, `>` and `&` so skill content can't forge internal markup.
/// Mirrors the sanitization Claude applies to synced skill descriptions.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_skill(root: &Path, tenant: Option<&str>, name: &str, content: &str) -> PathBuf {
        let dir = match tenant {
            Some(t) => root.join("tenants").join(t).join(name),
            None => root.join("platform").join(name),
        };
        fs::create_dir_all(&dir).unwrap();
        let mut f = fs::File::create(dir.join("SKILL.md")).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        dir
    }

    #[test]
    fn loads_enabled_and_filters() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(
            tmp.path(),
            None,
            "fmt",
            "---\nname: fmt\ndescription: Format code.\n---\nrun fmt\n",
        );
        write_skill(
            tmp.path(),
            Some("t1"),
            "fmt",
            "---\nname: fmt\ndescription: Tenant fmt.\n---\ntenant fmt\n",
        );
        write_skill(
            tmp.path(),
            Some("t1"),
            "lint",
            "---\nname: lint\ndescription: Lint.\n---\nrun lint\n",
        );
        // Same name in platform + tenant: platform wins (precedence).
        let loaded = load_skills(tmp.path(), Some("t1"), &["fmt".to_string()]);
        assert_eq!(loaded.len(), 1, "higher-priority layer wins");
        assert_eq!(loaded[0].name, "fmt");
        assert!(
            loaded[0].instructions.contains("run fmt"),
            "platform copy kept"
        );

        // Tenant-only skill resolves from the tenant layer.
        let tenant_only = load_skills(tmp.path(), Some("t1"), &["lint".to_string()]);
        assert_eq!(tenant_only.len(), 1);
        assert!(tenant_only[0].instructions.contains("run lint"));
    }

    #[test]
    fn wildcard_loads_all_in_scope() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(
            tmp.path(),
            Some("t"),
            "a",
            "---\nname: a\ndescription: A.\n---\naaa\n",
        );
        write_skill(
            tmp.path(),
            Some("t"),
            "b",
            "---\nname: b\ndescription: B.\n---\nbbb\n",
        );
        assert_eq!(
            load_skills(tmp.path(), Some("t"), &["*".to_string()]).len(),
            2
        );
        assert_eq!(load_skills(tmp.path(), Some("t"), &[]).len(), 0);
    }

    #[test]
    fn malformed_skill_is_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(tmp.path(), None, "broken", "no frontmatter\n");
        write_skill(
            tmp.path(),
            None,
            "ok",
            "---\nname: ok\ndescription: OK.\n---\nbody\n",
        );
        let loaded = load_skills(tmp.path(), None, &["*".to_string()]);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "ok");
    }

    #[test]
    fn render_full_vs_compact_and_escapes() {
        let s = LoadedSkill {
            name: "x".into(),
            description: "say <hi>".into(),
            instructions: "step 1".into(),
            always: false,
            tools: Vec::new(),
            disallowed_tools: Vec::new(),
            dir: PathBuf::new(),
        };
        let full = render_skills(std::slice::from_ref(&s), true).unwrap();
        assert!(full.contains("<instruction>step 1</instruction>"));
        let compact = render_skills(std::slice::from_ref(&s), false).unwrap();
        assert!(!compact.contains("<instruction>"));
        assert!(compact.contains("read_skill"));
        let always = LoadedSkill {
            always: true,
            ..s.clone()
        };
        assert!(
            render_skills(&[always], false)
                .unwrap()
                .contains("<instruction>")
        );
        let with_escape = LoadedSkill {
            description: "a<b>c".into(),
            ..s.clone()
        };
        assert!(
            render_skills(&[with_escape], true)
                .unwrap()
                .contains("a&lt;b&gt;c")
        );
    }
}

/// Return the instruction body of an enabled skill by name (used by
/// `read_skill`). Only enabled names (or `*`) resolve.
pub fn skill_text(
    root: &Path,
    tenant: Option<&str>,
    enabled: &[String],
    name: &str,
) -> Option<String> {
    if enabled.is_empty()
        || (!enabled.iter().any(|e| e == "*") && !enabled.iter().any(|e| e == name))
    {
        return None;
    }
    let mut layers: Vec<PathBuf> = vec![root.join("platform")];
    if let Some(t) = tenant {
        layers.push(root.join("tenants").join(t));
    }
    for layer in layers {
        let file = layer.join(name).join("SKILL.md");
        let Ok(raw) = fs::read_to_string(&file) else {
            continue;
        };
        if let Ok(doc) = SkillDocument::parse(&raw) {
            return Some(doc.body.trim().to_string());
        }
    }
    None
}

pub mod import;
