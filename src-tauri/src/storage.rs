use std::path::Path;
pub(crate) fn validate_config_path(path: &Path, home: Option<&Path>) -> Result<(), String> {
    // Keep the storage boundary error semantic so the IPC layer can localize it.
    // Never return translated text from this validation helper.
    let invalid = || "model_configuration_path".to_string();
    if !path.is_absolute()
        || path.file_name().is_none()
        || path
            .components()
            .any(|c| c == std::path::Component::ParentDir)
    {
        return Err(invalid());
    }
    // Resolve existing ancestors before checking repository boundaries. The
    // config's parent may not exist yet; save() creates it on first use.
    let parent = path.parent().ok_or_else(invalid)?;
    let mut resolved_parent = None;
    for ancestor in parent.ancestors() {
        match ancestor.canonicalize() {
            Ok(base) => {
                resolved_parent =
                    Some(base.join(parent.strip_prefix(ancestor).map_err(|_| invalid())?));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(invalid()),
        }
    }
    let parent = resolved_parent.ok_or_else(invalid)?;
    let home = home.and_then(|p| p.canonicalize().ok());
    let app_data = home.as_ref().is_some_and(|p| {
        parent.starts_with(p.join("Library/Application Support/com.memivy.phase1"))
            || parent.starts_with(p.join("Library/Application Support/com.memivy.app"))
    });
    // A HOME-level dotfiles repository must not block standard app storage.
    // Only exempt that exact marker, not a project/worktree inside app data.
    if parent
        .ancestors()
        .any(|p| p.join(".git").exists() && !(app_data && home.as_deref() == Some(p)))
    {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_config_path;

    #[test]
    fn rejected_repository_path_uses_a_stable_error_code() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("project");
        std::fs::create_dir_all(repository.join(".git")).unwrap();

        let error = validate_config_path(&repository.join("model.json"), None).unwrap_err();

        assert_eq!(error, "model_configuration_path");
    }
}
