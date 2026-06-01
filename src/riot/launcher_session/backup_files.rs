use super::*;

pub(super) fn replace_dir_contents(
    source_dir: impl AsRef<Path>,
    target_dir: impl AsRef<Path>,
) -> Result<(), LauncherSessionError> {
    let source_dir = source_dir.as_ref();
    let target_dir = target_dir.as_ref();

    if !source_dir.exists() {
        return Err(LauncherSessionError::SourceDataMissing(
            source_dir.to_path_buf(),
        ));
    }

    clear_dir(target_dir)?;
    copy_dir_contents(source_dir, target_dir)?;
    Ok(())
}

pub(super) fn clear_dir(path: &Path) -> Result<(), LauncherSessionError> {
    if !path.exists() {
        fs::create_dir_all(path)?;
        return Ok(());
    }

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            fs::remove_dir_all(path)?;
        } else {
            fs::remove_file(path)?;
        }
    }

    Ok(())
}

fn copy_dir_contents(source_dir: &Path, target_dir: &Path) -> Result<(), LauncherSessionError> {
    fs::create_dir_all(target_dir)?;

    for entry in fs::read_dir(source_dir)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target_dir.join(entry.file_name());

        if source_path.is_dir() {
            copy_dir_contents(&source_path, &target_path)?;
        } else {
            fs::copy(source_path, target_path)?;
        }
    }

    Ok(())
}
