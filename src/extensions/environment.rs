//! Environment policy for every extension process, including validation checks.
use crate::Result;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, ffi::OsStr, path::Path};
use tokio::process::Command;

const BASELINE: &[&str] = &[
    "PATH", "HOME", "TMPDIR", "LANG", "LC_ALL", "LC_CTYPE", "TZ", "TERM",
];
/// Windows programs, Node and Python among them, fail to start without these.
#[cfg(windows)]
const WINDOWS_BASELINE: &[&str] = &[
    "SystemRoot",
    "SystemDrive",
    "windir",
    "ComSpec",
    "PATHEXT",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "ProgramData",
    "TEMP",
    "TMP",
];

/// Only names are retained in manifests and registry snapshots. Values are
/// resolved at invocation time and never added to extension metadata.
#[derive(Default, Deserialize, Serialize)]
#[serde(try_from = "Vec<String>")]
pub(super) struct Environment(Vec<String>);

impl TryFrom<Vec<String>> for Environment {
    type Error = &'static str;

    fn try_from(names: Vec<String>) -> std::result::Result<Self, Self::Error> {
        let mut seen = HashSet::new();
        if names.len() > 32
            || names.iter().any(|name| {
                name.is_empty()
                    || name.len() > 128
                    || !name.bytes().enumerate().all(|(index, byte)| {
                        byte == b'_'
                            || byte.is_ascii_alphabetic()
                            || index > 0 && byte.is_ascii_digit()
                    })
                    || matches!(
                        name.as_str(),
                        "HIBISCUS_EXTENSION_DIR" | "HIBISCUS_WORKSPACE"
                    )
                    || !seen.insert(name)
            })
        {
            return Err("inheritEnv needs at most 32 unique variable names of 1–128 ASCII letters, digits or underscores, starting with a letter or underscore; Hibiscus context variables are reserved");
        }
        Ok(Self(names))
    }
}

impl Environment {
    pub(super) fn command(
        &self,
        program: impl AsRef<OsStr>,
        extension_dir: &Path,
        workspace: &Path,
    ) -> Result<Command> {
        let mut command = Command::new(program);
        command.env_clear();
        #[cfg(windows)]
        let baseline = BASELINE.iter().chain(WINDOWS_BASELINE);
        #[cfg(unix)]
        let baseline = BASELINE.iter();
        for name in baseline {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        for name in &self.0 {
            let value = std::env::var_os(name).ok_or_else(|| {
                format!("Declared extension environment variable is not set: {name}")
            })?;
            command.env(name, value);
        }
        command
            .env("HIBISCUS_EXTENSION_DIR", extension_dir)
            .env("HIBISCUS_WORKSPACE", workspace);
        Ok(command)
    }
}
