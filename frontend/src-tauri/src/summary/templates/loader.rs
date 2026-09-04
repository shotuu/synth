use super::defaults;
use super::types::Template;
use std::path::PathBuf;
use tracing::{debug, info, warn};
use once_cell::sync::Lazy;
use std::sync::RwLock;

// Global storage for the bundled templates directory path
static BUNDLED_TEMPLATES_DIR: Lazy<RwLock<Option<PathBuf>>> = Lazy::new(|| RwLock::new(None));

/// Set the bundled templates directory path (called once at app startup)
pub fn set_bundled_templates_dir(path: PathBuf) {
    info!("Bundled templates directory set to: {:?}", path);
    if let Ok(mut dir) = BUNDLED_TEMPLATES_DIR.write() {
        *dir = Some(path);
    }
}

/// Get the user's custom templates directory path
///
/// Returns the platform-specific application data directory for custom templates:
/// - macOS: ~/Library/Application Support/Synth/templates/
/// - Windows: %APPDATA%\Synth\templates\
/// - Linux: ~/.config/Synth/templates/
pub fn get_custom_templates_dir() -> Option<PathBuf> {
    let mut path = dirs::data_dir()?;
    path.push("Synth");
    path.push("templates");
    Some(path)
}

/// Save a custom template's JSON to the user's custom templates directory,
/// validating it first so a malformed template never lands on disk. The
/// filename doubles as the template's id in list_template_ids/get_template.
pub fn save_custom_template(id: &str, json_content: &str) -> Result<(), String> {
    if id.trim().is_empty() || id.contains(['/', '\\', '.']) {
        return Err("Template id must be non-empty and contain no path separators".to_string());
    }
    validate_and_parse_template(json_content)?;

    let dir = get_custom_templates_dir()
        .ok_or_else(|| "Could not resolve custom templates directory".to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create templates directory: {}", e))?;

    let path = dir.join(format!("{}.json", id));
    std::fs::write(&path, json_content).map_err(|e| format!("Failed to write template: {}", e))?;
    info!("Saved custom template '{}' to {:?}", id, path);
    Ok(())
}

/// Delete a custom template. Built-in templates can't be deleted this way
/// since they aren't in this directory -- deleting a nonexistent file is
/// reported as "not found" rather than silently succeeding.
pub fn delete_custom_template(id: &str) -> Result<bool, String> {
    if id.trim().is_empty() || id.contains(['/', '\\', '.']) {
        return Err("Template id must be non-empty and contain no path separators".to_string());
    }

    let dir = get_custom_templates_dir()
        .ok_or_else(|| "Could not resolve custom templates directory".to_string())?;
    let path = dir.join(format!("{}.json", id));

    if !path.exists() {
        return Ok(false);
    }
    std::fs::remove_file(&path).map_err(|e| format!("Failed to delete template: {}", e))?;
    info!("Deleted custom template '{}'", id);
    Ok(true)
}

/// Load a template from the bundled resources directory
///
/// # Arguments
/// * `template_id` - Template identifier (without .json extension)
///
/// # Returns
/// The template JSON content if found, None otherwise
fn load_bundled_template(template_id: &str) -> Option<String> {
    let bundled_dir = BUNDLED_TEMPLATES_DIR.read().ok()?.clone()?;
    let template_path = bundled_dir.join(format!("{}.json", template_id));

    debug!("Checking for bundled template at: {:?}", template_path);

    match std::fs::read_to_string(&template_path) {
        Ok(content) => {
            info!("Loaded bundled template '{}' from {:?}", template_id, template_path);
            Some(content)
        }
        Err(e) => {
            debug!("No bundled template '{}' found: {}", template_id, e);
            None
        }
    }
}

/// Load a template from the user's custom templates directory
///
/// # Arguments
/// * `template_id` - Template identifier (without .json extension)
///
/// # Returns
/// The template JSON content if found, None otherwise
fn load_custom_template(template_id: &str) -> Option<String> {
    let custom_dir = get_custom_templates_dir()?;
    let template_path = custom_dir.join(format!("{}.json", template_id));

    debug!("Checking for custom template at: {:?}", template_path);

    match std::fs::read_to_string(&template_path) {
        Ok(content) => {
            info!("Loaded custom template '{}' from {:?}", template_id, template_path);
            Some(content)
        }
        Err(e) => {
            debug!("No custom template '{}' found: {}", template_id, e);
            None
        }
    }
}

/// Load and parse a template by identifier
///
/// This function implements a fallback strategy:
/// 1. Check user's custom templates directory
/// 2. Check bundled resources directory (app templates)
/// 3. Fall back to built-in embedded templates
/// 4. Return error if not found in any location
///
/// # Arguments
/// * `template_id` - Template identifier (e.g., "daily_standup", "standard_meeting")
///
/// # Returns
/// Parsed and validated Template struct
pub fn get_template(template_id: &str) -> Result<Template, String> {
    info!("Loading template: {}", template_id);

    // Try custom template first, then bundled, then built-in
    let json_content = if let Some(custom_content) = load_custom_template(template_id) {
        debug!("Using custom template for '{}'", template_id);
        custom_content
    } else if let Some(bundled_content) = load_bundled_template(template_id) {
        debug!("Using bundled template for '{}'", template_id);
        bundled_content
    } else if let Some(builtin_content) = defaults::get_builtin_template(template_id) {
        debug!("Using built-in template for '{}'", template_id);
        builtin_content.to_string()
    } else {
        return Err(format!(
            "Template '{}' not found. Available templates: {}",
            template_id,
            list_template_ids().join(", ")
        ));
    };

    // Parse and validate
    validate_and_parse_template(&json_content)
}

/// Validate and parse template JSON
///
/// # Arguments
/// * `json_content` - Raw JSON string
///
/// # Returns
/// Parsed and validated Template struct
pub fn validate_and_parse_template(json_content: &str) -> Result<Template, String> {
    let template: Template = serde_json::from_str(json_content)
        .map_err(|e| format!("Failed to parse template JSON: {}", e))?;

    template.validate()?;

    Ok(template)
}

/// List all available template identifiers
///
/// Returns a combined list of:
/// - Built-in template IDs
/// - Bundled template IDs (from app resources)
/// - Custom template IDs (from user's data directory)
pub fn list_template_ids() -> Vec<String> {
    let mut ids: Vec<String> = defaults::list_builtin_template_ids()
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    // Add bundled templates if directory is set
    if let Ok(bundled_dir_lock) = BUNDLED_TEMPLATES_DIR.read() {
        if let Some(bundled_dir) = bundled_dir_lock.as_ref() {
            if bundled_dir.exists() {
                match std::fs::read_dir(bundled_dir) {
                    Ok(entries) => {
                        for entry in entries.flatten() {
                            if let Some(filename) = entry.file_name().to_str() {
                                if filename.ends_with(".json") {
                                    let id = filename.trim_end_matches(".json").to_string();
                                    if !ids.contains(&id) {
                                        ids.push(id);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to read bundled templates directory: {}", e);
                    }
                }
            }
        }
    }

    // Add custom templates if directory exists
    if let Some(custom_dir) = get_custom_templates_dir() {
        if custom_dir.exists() {
            match std::fs::read_dir(&custom_dir) {
                Ok(entries) => {
                    for entry in entries.flatten() {
                        if let Some(filename) = entry.file_name().to_str() {
                            if filename.ends_with(".json") {
                                let id = filename.trim_end_matches(".json").to_string();
                                if !ids.contains(&id) {
                                    ids.push(id);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to read custom templates directory: {}", e);
                }
            }
        }
    }

    ids.sort();
    ids
}

/// List all available templates with their metadata
///
/// Returns a list of (id, name, description) tuples
pub fn list_templates() -> Vec<(String, String, String)> {
    let mut templates = Vec::new();

    for id in list_template_ids() {
        match get_template(&id) {
            Ok(template) => {
                templates.push((id, template.name, template.description));
            }
            Err(e) => {
                warn!("Failed to load template '{}': {}", id, e);
            }
        }
    }

    templates
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Touches the real custom templates directory (it isn't mockable in
    /// this design), so a unique id and explicit cleanup on both ends keep
    /// this test from leaving litter or colliding with a parallel test run.
    #[test]
    fn save_get_and_delete_custom_template_round_trip() {
        let id = "synth_test_template_save_get_delete";
        let _ = delete_custom_template(id); // defensive: clear any leftover from a prior failed run

        let json = r#"{
            "name": "My Custom Template",
            "description": "A test template",
            "sections": [
                {"title": "Summary", "instruction": "Summarize it", "format": "paragraph"}
            ]
        }"#;

        save_custom_template(id, json).expect("save should succeed for valid JSON");

        let loaded = get_template(id).expect("should load the just-saved template");
        assert_eq!(loaded.name, "My Custom Template");
        assert_eq!(loaded.sections.len(), 1);

        assert!(list_template_ids().contains(&id.to_string()));

        let deleted = delete_custom_template(id).expect("delete should succeed");
        assert!(deleted);
        assert!(get_template(id).is_err(), "template should be gone after delete");

        let deleted_again = delete_custom_template(id).expect("deleting a missing template is not an error");
        assert!(!deleted_again, "second delete should report nothing was there");
    }

    #[test]
    fn save_custom_template_rejects_invalid_json() {
        let id = "synth_test_template_invalid";
        let result = save_custom_template(id, "not valid json");
        assert!(result.is_err());
        // Must not have written anything for a template that failed validation
        assert!(get_template(id).is_err());
    }

    #[test]
    fn save_custom_template_rejects_path_traversal_ids() {
        let json = r#"{"name":"x","description":"y","sections":[{"title":"S","instruction":"i","format":"paragraph"}]}"#;
        assert!(save_custom_template("../evil", json).is_err());
        assert!(save_custom_template("nested/path", json).is_err());
        assert!(save_custom_template("", json).is_err());
    }

    #[test]
    fn delete_custom_template_rejects_path_traversal_ids() {
        assert!(delete_custom_template("../evil").is_err());
        assert!(delete_custom_template("nested/path").is_err());
        assert!(delete_custom_template("").is_err());
    }

    #[test]
    fn test_get_builtin_template() {
        let template = get_template("daily_standup");
        assert!(template.is_ok());

        let template = template.unwrap();
        assert_eq!(template.name, "Daily Standup");
        assert!(!template.sections.is_empty());
    }

    #[test]
    fn test_get_nonexistent_template() {
        let result = get_template("nonexistent_template");
        assert!(result.is_err());
    }

    #[test]
    fn test_list_template_ids() {
        let ids = list_template_ids();
        assert!(ids.contains(&"daily_standup".to_string()));
        assert!(ids.contains(&"standard_meeting".to_string()));
    }

    #[test]
    fn test_validate_invalid_json() {
        let result = validate_and_parse_template("invalid json");
        assert!(result.is_err());
    }
}
