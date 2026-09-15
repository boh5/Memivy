use std::path::{Path, PathBuf};

pub(crate) const DEVELOPMENT_IDENTIFIER: &str = "com.memivy.app.dev";

fn resolved_path(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| part == std::path::Component::ParentDir)
    {
        return Err("The library path must be absolute without parent traversal.".into());
    }
    for ancestor in path.ancestors() {
        match ancestor.canonicalize() {
            Ok(base) => return Ok(base.join(path.strip_prefix(ancestor).unwrap())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("Could not resolve the library path: {error}")),
        }
    }
    Err("Could not resolve the library path.".into())
}

pub(crate) fn application_root(
    identifier: &str,
    home: &Path,
    override_path: Option<&Path>,
) -> Result<PathBuf, String> {
    let development = identifier == DEVELOPMENT_IDENTIFIER;
    let default = home
        .join("Library/Application Support")
        .join(if development {
            DEVELOPMENT_IDENTIFIER
        } else {
            "com.memivy.app"
        });
    let selected = resolved_path(override_path.unwrap_or(&default))?;
    if development {
        let production = resolved_path(&home.join("Library/Application Support/com.memivy.app"))?;
        // Reserve case aliases of these ASCII library paths before they exist.
        // Matching shared components means one directory contains the other.
        if selected
            .components()
            .zip(production.components())
            .all(|(left, right)| {
                left.as_os_str()
                    .as_encoded_bytes()
                    .eq_ignore_ascii_case(right.as_os_str().as_encoded_bytes())
            })
        {
            return Err(
                "Memivy Dev cannot open the production library or its parent directories.".into(),
            );
        }
    }
    Ok(selected)
}

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
        ["com.memivy.app", DEVELOPMENT_IDENTIFIER]
            .iter()
            .any(|id| parent.starts_with(p.join("Library/Application Support").join(id)))
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
    use super::*;

    #[test]
    fn installed_and_development_libraries_are_separate() {
        let home = tempfile::tempdir().unwrap();
        let production = application_root("com.memivy.app", home.path(), None).unwrap();
        let development = application_root(DEVELOPMENT_IDENTIFIER, home.path(), None).unwrap();
        assert_ne!(production, development);
        assert!(development.ends_with("com.memivy.app.dev"));
        assert!(application_root(DEVELOPMENT_IDENTIFIER, home.path(), Some(&production)).is_err());
        assert!(application_root(DEVELOPMENT_IDENTIFIER, home.path(), Some(home.path())).is_err());
    }

    #[test]
    fn development_rejects_symlinked_production_and_relative_libraries() {
        let home = tempfile::tempdir().unwrap();
        let production = home
            .path()
            .join("Library/Application Support/com.memivy.app");
        std::fs::create_dir_all(&production).unwrap();
        let alias = home.path().join("alias");
        std::os::unix::fs::symlink(&production, &alias).unwrap();
        for path in [alias, production.join("qa"), PathBuf::from("relative")] {
            assert!(application_root(DEVELOPMENT_IDENTIFIER, home.path(), Some(&path)).is_err());
        }
        let isolated = home.path().join("isolated");
        assert!(application_root(DEVELOPMENT_IDENTIFIER, home.path(), Some(&isolated)).is_ok());
    }

    #[test]
    fn development_rejects_case_aliases_before_production_exists() {
        let home = tempfile::tempdir().unwrap();
        let alias = home
            .path()
            .join("Library/Application Support/COM.MEMIVY.APP");
        for path in [alias.clone(), alias.join("qa"), home.path().join("LIBRARY")] {
            assert!(application_root(DEVELOPMENT_IDENTIFIER, home.path(), Some(&path)).is_err());
        }
    }

    #[test]
    fn rejected_repository_path_uses_a_stable_error_code() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("project");
        std::fs::create_dir_all(repository.join(".git")).unwrap();

        let error = validate_config_path(&repository.join("model.json"), None).unwrap_err();

        assert_eq!(error, "model_configuration_path");
    }
}

#[cfg(test)]
mod configuration_boundary_tests {
    use super::*;
    use memivy_core::model::ModelConfig;
    use std::{fs, os::unix::fs::PermissionsExt};

    #[test]
    fn home_dotfiles_repository_allows_app_settings_to_save_and_reopen() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir(home.path().join(".git")).unwrap();
        let path = home
            .path()
            .join("Library/Application Support/com.memivy.app/model.json");
        validate_config_path(&path, Some(home.path())).unwrap();
        let config = ModelConfig {
            provider: Default::default(),
            base_url: "http://127.0.0.1:11435/v1".into(),
            model: "test-only".into(),
            api_key: Some("synthetic-test-key".into()),
            max_output_tokens: None,
            output_token_parameter: Default::default(),
            disable_reasoning: false,
        };
        config.save(&path).unwrap();
        assert_eq!(ModelConfig::read(&path).unwrap().api_key, config.api_key);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        validate_config_path(&path, Some(home.path())).unwrap();
    }

    #[test]
    fn repositories_and_worktrees_inside_app_data_are_still_rejected() {
        let home = tempfile::tempdir().unwrap();
        let app_data = home
            .path()
            .join("Library/Application Support/com.memivy.app");
        for repo in [home.path().join("Developer/project"), app_data] {
            fs::create_dir_all(&repo).unwrap();
            // Linked worktrees use a .git file rather than a directory.
            fs::write(repo.join(".git"), "gitdir: /unused-test-path").unwrap();
            assert!(
                validate_config_path(&repo.join("research/model.json"), Some(home.path())).is_err()
            );
        }
        fs::create_dir(home.path().join(".git")).unwrap();
        assert!(validate_config_path(&home.path().join("model.json"), Some(home.path())).is_err());
    }

    #[test]
    fn symlink_into_repository_cannot_use_the_app_data_exception() {
        let home = tempfile::tempdir().unwrap();
        let repo = home.path().join("Developer/project");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir(repo.join("research")).unwrap();
        let support = home.path().join("Library/Application Support");
        fs::create_dir_all(&support).unwrap();
        std::os::unix::fs::symlink(repo.join("research"), support.join("com.memivy.app")).unwrap();
        assert!(
            validate_config_path(
                &support.join("com.memivy.app/model.json"),
                Some(home.path())
            )
            .is_err()
        );
    }

    #[test]
    fn external_config_accepts_new_directories_but_not_relative_or_parent_paths() {
        let dir = tempfile::tempdir().unwrap();
        assert!(validate_config_path(&dir.path().join("new/private/model.json"), None).is_ok());
        assert!(validate_config_path(Path::new("model.json"), None).is_err());
        assert!(validate_config_path(&dir.path().join("new/../model.json"), None).is_err());
    }
}
