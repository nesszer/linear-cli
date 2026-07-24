use anyhow::{Context, Result};
use clap::{Subcommand, ValueHint};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::api::{parse_linear_upload_url, LinearClient};

#[cfg(unix)]
fn create_private_file(path: &Path) -> Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("Failed to create file: {}", path.display()))
}

#[cfg(not(unix))]
fn create_private_file(path: &Path) -> Result<std::fs::File> {
    std::fs::File::create(path)
        .with_context(|| format!("Failed to create file: {}", path.display()))
}

fn atomic_temp_path(path: &Path) -> Result<PathBuf> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("Output path must include a file name")?;
    let unique = format!(
        ".{}.tmp-{}-{}",
        file_name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    );
    Ok(parent.join(unique))
}

struct AtomicPrivateFile {
    file: Option<std::fs::File>,
    temp_path: PathBuf,
    final_path: PathBuf,
    committed: bool,
}

impl AtomicPrivateFile {
    fn create(path: &Path) -> Result<Self> {
        let temp_path = atomic_temp_path(path)?;
        let file = create_private_file(&temp_path)
            .with_context(|| format!("Failed to create file: {}", temp_path.display()))?;
        Ok(Self {
            file: Some(file),
            temp_path,
            final_path: path.to_path_buf(),
            committed: false,
        })
    }

    fn commit(&mut self) -> Result<()> {
        if self.committed {
            return Ok(());
        }

        if let Some(mut file) = self.file.take() {
            file.flush()?;
            file.sync_all()?;
            drop(file);
        }

        std::fs::rename(&self.temp_path, &self.final_path)?;
        self.committed = true;
        Ok(())
    }

    #[cfg(test)]
    fn file_mut(&mut self) -> &mut std::fs::File {
        self.file.as_mut().expect("atomic file should be open")
    }
}

impl Write for AtomicPrivateFile {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.file
            .as_mut()
            .expect("atomic file should be open")
            .write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file
            .as_mut()
            .expect("atomic file should be open")
            .flush()
    }
}

impl Drop for AtomicPrivateFile {
    fn drop(&mut self) {
        if !self.committed {
            let _ = self.file.take();
            let _ = std::fs::remove_file(&self.temp_path);
        }
    }
}

#[derive(Subcommand)]
pub enum UploadCommands {
    /// Fetch an upload from Linear's upload storage
    #[command(alias = "get")]
    Fetch {
        /// The Linear upload URL (e.g., https://uploads.linear.app/...)
        url: String,

        /// Output file path (if not specified, outputs to stdout)
        #[arg(short = 'f', long = "file", value_hint = ValueHint::FilePath)]
        file: Option<String>,
    },
}

pub async fn handle(cmd: UploadCommands) -> Result<()> {
    match cmd {
        UploadCommands::Fetch { url, file } => fetch_upload(&url, file).await,
    }
}

async fn fetch_upload(url: &str, file: Option<String>) -> Result<()> {
    parse_linear_upload_url(url)?;

    let client = LinearClient::new()?;

    if let Some(file_path) = file {
        // Stream to a sibling temp file and rename into place on success.
        let mut file = AtomicPrivateFile::create(Path::new(&file_path))?;
        let bytes_written = client
            .fetch_to_writer(url, &mut file)
            .await
            .context("Failed to fetch upload from Linear")?;
        file.commit()?;
        eprintln!("Downloaded {} bytes to {}", bytes_written, file_path);
    } else {
        // Stream directly to stdout
        let mut stdout_handle = io::stdout().lock();
        let bytes_written = client
            .fetch_to_writer(url, &mut stdout_handle)
            .await
            .context("Failed to fetch upload from Linear")?;
        stdout_handle.flush()?;
        drop(stdout_handle);
        eprintln!("Downloaded {} bytes", bytes_written);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_path(label: &str) -> PathBuf {
        let unique = format!(
            "linear-cli-upload-{}-{}-{}",
            label,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::env::temp_dir().join(unique)
    }

    #[test]
    fn test_atomic_private_file_commit_replaces_destination() {
        let path = temp_path("commit");
        std::fs::write(&path, "old").unwrap();

        let mut file = AtomicPrivateFile::create(&path).unwrap();
        file.file_mut().write_all(b"new").unwrap();
        file.commit().unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    }

    #[test]
    fn test_atomic_private_file_drop_preserves_existing_destination() {
        let path = temp_path("drop");
        std::fs::write(&path, "old").unwrap();

        let mut file = AtomicPrivateFile::create(&path).unwrap();
        file.file_mut().write_all(b"partial").unwrap();
        drop(file);

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
    }
}
