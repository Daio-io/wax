use std::path::{Path, PathBuf};

use wax_core::paths::{PathsError, state_file};

pub(crate) fn resolve_state_path(override_path: Option<&Path>) -> Result<PathBuf, PathsError> {
    match override_path {
        Some(path) => Ok(path.to_path_buf()),
        None => state_file(),
    }
}
