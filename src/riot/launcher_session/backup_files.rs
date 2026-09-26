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

    // Copy everything next to the target first, so a failed copy (for example a file Riot Client
    // still has locked) leaves the existing folder untouched.
    let incoming_dir = sibling_dir(target_dir, "incoming");
    let previous_dir = sibling_dir(target_dir, "previous");
    remove_dir_if_exists(&incoming_dir)?;

    if let Err(error) = copy_dir_contents(source_dir, &incoming_dir) {
        let _ = fs::remove_dir_all(&incoming_dir);
        return Err(error);
    }

    if target_dir.exists() {
        remove_dir_if_exists(&previous_dir)?;

        if fs::rename(target_dir, &previous_dir).is_err() {
            // Something still holds the target folder open, so it cannot be swapped out. Refill it
            // from the complete copy instead.
            let result =
                clear_dir(target_dir).and_then(|()| copy_dir_contents(&incoming_dir, target_dir));
            let _ = fs::remove_dir_all(&incoming_dir);
            return result;
        }
    }

    if let Err(error) = fs::rename(&incoming_dir, target_dir) {
        if previous_dir.exists() {
            let _ = fs::rename(&previous_dir, target_dir);
        }
        let _ = fs::remove_dir_all(&incoming_dir);
        return Err(error.into());
    }

    let _ = remove_dir_if_exists(&previous_dir);
    Ok(())
}

fn sibling_dir(dir: &Path, suffix: &str) -> PathBuf {
    let name = dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    dir.with_file_name(format!("{name}.{suffix}"))
}

fn remove_dir_if_exists(dir: &Path) -> io::Result<()> {
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }

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
