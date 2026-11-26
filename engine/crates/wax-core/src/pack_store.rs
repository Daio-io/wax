//! Installation and lookup of language packs in a Wax home.

use std::path::{Path, PathBuf};
use std::{fs, io};

use serde::Deserialize;

use thiserror::Error;
use wax_contract::LanguageId;

use crate::advisory_lock::AdvisoryLock;
use crate::auto_install::InstalledManifest;
use crate::global_state::{
    GlobalState, GlobalStateError, InstalledLanguagePack, load_global_state, save_global_state,
};
use crate::install::{InstallError, LanguagePackManifestSpec, install_language};
use crate::paths::{PathsError, WaxHome};

/// Language packs managed within one Wax home.
pub struct PackStore {
    home: WaxHome,
}

/// Artifact pin and manifest metadata for a language pack.
pub struct PackArtifact<'a> {
    /// Target triple for the artifact.
    pub target: &'a str,
    /// Download URL for the archive.
    pub url: &'a str,
    /// Expected SHA-256 digest of the archive.
    pub sha256: &'a str,
    /// Digest from the pack index, when available.
    pub pack_index_sha256: Option<&'a str>,
    /// Wire API version to record in the manifest.
    pub api_version: u32,
}

/// A pack on disk and the command used to launch it.
#[derive(Debug)]
pub struct InstalledPack {
    /// Directory containing the manifest and bundled files.
    pub install_dir: PathBuf,
    /// Launch arguments with an install-relative executable resolved.
    pub command: Vec<String>,
}

/// Failure to install, look up, or remove a language pack.
#[derive(Debug, Error)]
pub enum PackStoreError {
    /// Artifact installation failed.
    #[error(transparent)]
    Install(#[from] InstallError),
    /// Global state could not be read or written.
    #[error(transparent)]
    GlobalState(#[from] GlobalStateError),
    /// An install directory could not be resolved.
    #[error(transparent)]
    Paths(#[from] PathsError),
    /// An installed manifest could not be read or decoded.
    #[error("failed to load installed manifest at {path}: {source}")]
    ManifestRead {
        /// Manifest file path.
        path: String,
        /// Underlying file or JSON error.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// The requested version is absent or the installed manifest identifies a different pack.
    #[error(
        "installed manifest mismatch for language {language_id}: expected version {expected_version}"
    )]
    ManifestMismatch {
        /// Language being looked up.
        language_id: LanguageId,
        /// Version being looked up.
        expected_version: String,
    },
    /// An install directory could not be removed.
    #[error("failed to clean up installed language pack at {path}: {source}")]
    Cleanup {
        /// Directory being removed.
        path: String,
        /// Underlying filesystem error.
        #[source]
        source: io::Error,
    },
}

impl PackStore {
    /// Opens a pack store rooted at the given Wax home.
    pub fn new(home: WaxHome) -> Self {
        Self { home }
    }

    /// Installs or reuses a language pack and records it in global state.
    /// Removes a fresh install if state cannot be recorded.
    ///
    /// # Errors
    ///
    /// Returns an error if artifact installation or state persistence fails.
    pub fn install(
        &self,
        id: &LanguageId,
        version: &str,
        artifact: PackArtifact<'_>,
    ) -> Result<InstalledPack, PackStoreError> {
        let _lock = self.lock_mutations()?;
        let binary = format!("wax-lang-{id}");
        let manifest = LanguagePackManifestSpec {
            id: id.clone(),
            version: version.to_owned(),
            api_version: artifact.api_version,
            command: vec![format!("./{binary}"), "--stdio".to_owned()],
            ecosystem: id.to_string(),
            parser_name: id.to_string(),
            parser_version: version.to_owned(),
        };
        let installed = install_language(
            id,
            version,
            artifact.target,
            artifact.url,
            artifact.sha256,
            artifact.pack_index_sha256,
            &manifest,
            &self.home,
        );
        let (install_dir, command) = match installed {
            Ok(dir) => (dir, Some(manifest.command)),
            Err(InstallError::AlreadyInstalled { .. }) => {
                (self.home.lang_install_dir(id, version)?, None)
            }
            Err(error) => return Err(error.into()),
        };
        let fresh = command.is_some();
        let result = (|| {
            let state_path = self.home.state_file();
            let mut state = load_global_state(&state_path)?;
            let command = match command {
                Some(command) => command,
                None => {
                    InstalledManifestFile::load(&install_dir)?
                        .verify(id, version)?
                        .command
                }
            };
            state
                .installed_languages
                .entry(id.clone())
                .or_default()
                .insert(
                    version.to_owned(),
                    InstalledLanguagePack {
                        install_dir: install_dir.clone(),
                    },
                );
            save_global_state(state_path, &state)?;
            Ok(command)
        })();
        let command = match result {
            Ok(command) => command,
            Err(error) => {
                if fresh {
                    remove_install_dir(&install_dir)?;
                }
                return Err(error);
            }
        };

        Ok(InstalledPack {
            command: resolve_manifest_command(&install_dir, command),
            install_dir,
        })
    }

    /// Looks up a recorded pack and resolves its manifest command.
    ///
    /// # Errors
    ///
    /// Returns an error if state cannot be read, the pack is missing, or its
    /// manifest cannot be read or belongs to another pack.
    pub fn installed(
        &self,
        id: &LanguageId,
        version: &str,
    ) -> Result<InstalledPack, PackStoreError> {
        let state = load_global_state(self.home.state_file())?;
        let (install_dir, manifest) = Self::installed_manifest(&state, id, version)?;
        Ok(InstalledPack {
            command: resolve_manifest_command(install_dir, manifest.command),
            install_dir: install_dir.to_owned(),
        })
    }

    pub(crate) fn installed_manifest<'a>(
        state: &'a GlobalState,
        id: &LanguageId,
        version: &str,
    ) -> Result<(&'a Path, InstalledManifestFile), PackStoreError> {
        let pack = state
            .installed_languages
            .get(id)
            .and_then(|versions| versions.get(version))
            .ok_or_else(|| manifest_mismatch(id, version))?;
        let manifest = InstalledManifestFile::load(&pack.install_dir)?.verify(id, version)?;
        Ok((&pack.install_dir, manifest))
    }

    /// Removes the given recorded versions and their managed directories.
    ///
    /// # Errors
    ///
    /// Returns an error if a managed directory cannot be resolved or removed, or
    /// global state cannot be read or saved.
    pub fn remove(&self, id: &LanguageId, versions: &[String]) -> Result<(), PackStoreError> {
        if versions.is_empty() {
            return Ok(());
        }
        let _lock = self.lock_mutations()?;
        let state_path = self.home.state_file();
        let mut state = load_global_state(&state_path)?;
        for version in versions {
            let Some(installed) = state.installed_languages.get_mut(id) else {
                break;
            };
            if !installed.contains_key(version) {
                continue;
            }

            remove_install_dir(&self.home.lang_install_dir(id, version)?)?;
            installed.remove(version);
            if installed.is_empty() {
                state.installed_languages.remove(id);
            }
            save_global_state(&state_path, &state)?;
        }
        Ok(())
    }

    fn lock_mutations(&self) -> Result<AdvisoryLock, InstallError> {
        let directory = self.home.root().join("langs");
        let lock_path = directory.join(".lock");
        // Hold the lock through cleanup so another operation cannot record a
        // directory that this one is about to remove.
        fs::create_dir_all(&directory)
            .and_then(|()| AdvisoryLock::acquire(&lock_path))
            .map_err(|source| InstallError::Io {
                context: format!("lock language pack store at {}", lock_path.display()),
                source,
            })
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct InstalledManifestFile {
    id: LanguageId,
    command: Vec<String>,
    #[serde(flatten)]
    pub(crate) metadata: InstalledManifest,
}

impl InstalledManifestFile {
    pub(crate) fn load(install_dir: &Path) -> Result<Self, PackStoreError> {
        let path = install_dir.join("manifest.json");
        let raw = fs::read_to_string(&path).map_err(|source| PackStoreError::ManifestRead {
            path: path.display().to_string(),
            source: Box::new(source),
        })?;
        serde_json::from_str(&raw).map_err(|source| PackStoreError::ManifestRead {
            path: path.display().to_string(),
            source: Box::new(source),
        })
    }

    fn verify(self, id: &LanguageId, version: &str) -> Result<Self, PackStoreError> {
        if self.id != *id || self.metadata.version != version {
            return Err(manifest_mismatch(id, version));
        }
        Ok(self)
    }
}

fn manifest_mismatch(id: &LanguageId, version: &str) -> PackStoreError {
    PackStoreError::ManifestMismatch {
        language_id: id.clone(),
        expected_version: version.to_owned(),
    }
}

fn resolve_manifest_command(install_dir: &Path, mut command: Vec<String>) -> Vec<String> {
    if let Some(primary) = command.first_mut()
        && let Some(relative) = primary.strip_prefix("./")
    {
        *primary = install_dir.join(relative).display().to_string();
    }
    command
}

fn remove_install_dir(path: &Path) -> Result<(), PackStoreError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(PackStoreError::Cleanup {
            path: path.display().to_string(),
            source,
        }),
    }
}
