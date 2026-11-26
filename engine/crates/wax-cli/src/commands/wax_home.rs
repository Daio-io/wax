use wax_core::paths::{PathsError, WaxHome};

pub(crate) fn resolve_wax_home(home: Option<&WaxHome>) -> Result<WaxHome, PathsError> {
    home.cloned().map_or_else(WaxHome::from_env, Ok)
}
