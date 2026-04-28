//! Skills system: markdown-based prompt templates.
//! Skills are .md files with YAML frontmatter (name, description).
//! Located in `skills/` directory relative to the working directory.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub content: String,
    pub file_path: String,
}

/// Discover skills from the skills/ directory.
pub fn discover_skills(base_dir: &Path) -> Vec<Skill> {
    let skills_dir = base_dir.join("skills");
    if !skills_dir.exists() {
        return Vec::new();
    }

    let mut skills = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&skills_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                if let Some(skill) = parse_skill_file(&path) {
                    skills.push(skill);
                }
            }
        }
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// Parse a skill markdown file with YAML frontmatter.
fn parse_skill_file(path: &Path) -> Option<Skill> {
    let content = std::fs::read_to_string(path).ok()?;

    // Parse YAML frontmatter between --- delimiters
    if !content.starts_with("---") {
        return None;
    }
    let end = content[3..].find("---")?;
    let frontmatter = &content[3..3 + end].trim();
    let body = content[3 + end + 3..].trim().to_string();

    // Simple YAML parsing (name and description fields)
    let mut name = None;
    let mut description = None;
    for line in frontmatter.lines() {
        if let Some(val) = line.strip_prefix("name:") {
            name = Some(val.trim().trim_matches('"').trim_matches('\'').to_string());
        }
        if let Some(val) = line.strip_prefix("description:") {
            description = Some(val.trim().trim_matches('"').trim_matches('\'').to_string());
        }
    }

    let name = name.or_else(|| {
        path.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string())
    })?;

    Some(Skill {
        name,
        description: description.unwrap_or_default(),
        content: body,
        file_path: path.to_string_lossy().to_string(),
    })
}

/// Get a skill by name.
pub fn get_skill(base_dir: &Path, name: &str) -> Option<Skill> {
    discover_skills(base_dir).into_iter().find(|s| s.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_parse_skill_file_valid() {
        let dir = std::env::temp_dir().join("skill_test_valid");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test-skill.md");
        fs::write(&path, "---\nname: test-skill\ndescription: A test skill\n---\n\nDo the thing at {{path}}\n").unwrap();

        let skill = parse_skill_file(&path).unwrap();
        assert_eq!(skill.name, "test-skill");
        assert_eq!(skill.description, "A test skill");
        assert!(skill.content.contains("{{path}}"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parse_skill_file_no_frontmatter() {
        let dir = std::env::temp_dir().join("skill_test_no_fm");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("bad.md");
        fs::write(&path, "Just some text without frontmatter").unwrap();

        assert!(parse_skill_file(&path).is_none());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_discover_skills_empty() {
        let dir = std::env::temp_dir().join("skill_test_empty");
        let _ = fs::create_dir_all(&dir);
        // No skills/ subdirectory
        let skills = discover_skills(&dir);
        assert!(skills.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_discover_skills_finds_files() {
        let dir = std::env::temp_dir().join("skill_test_discover");
        let skills_dir = dir.join("skills");
        let _ = fs::create_dir_all(&skills_dir);
        fs::write(skills_dir.join("alpha.md"), "---\nname: alpha\ndescription: First\n---\nContent A").unwrap();
        fs::write(skills_dir.join("beta.md"), "---\nname: beta\ndescription: Second\n---\nContent B").unwrap();

        let skills = discover_skills(&dir);
        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].name, "alpha");
        assert_eq!(skills[1].name, "beta");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_get_skill_by_name() {
        let dir = std::env::temp_dir().join("skill_test_get");
        let skills_dir = dir.join("skills");
        let _ = fs::create_dir_all(&skills_dir);
        fs::write(skills_dir.join("my-skill.md"), "---\nname: my-skill\ndescription: Test\n---\nBody here").unwrap();

        let skill = get_skill(&dir, "my-skill");
        assert!(skill.is_some());
        assert_eq!(skill.unwrap().content, "Body here");

        assert!(get_skill(&dir, "nonexistent").is_none());

        let _ = fs::remove_dir_all(&dir);
    }
}
