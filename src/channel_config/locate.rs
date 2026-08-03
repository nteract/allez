//! `.condarc` file-reading internals.

use std::{io, path::Path};

pub(super) enum ReadOutcome {
    Missing,
    Unreadable(io::Error),
}

pub(super) fn read_condarc(path: &Path) -> Result<String, ReadOutcome> {
    std::fs::read_to_string(path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => ReadOutcome::Missing,
        _ => ReadOutcome::Unreadable(error),
    })
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};

    use tempfile::NamedTempFile;

    use super::{ReadOutcome, read_condarc};

    #[test]
    fn read_condarc_nonexistent_path_returns_missing() {
        // Given
        let temporary_directory = tempfile::tempdir().unwrap();
        let path = temporary_directory.path().join("missing.condarc");

        // When
        let result = read_condarc(&path);

        // Then
        assert!(matches!(result, Err(ReadOutcome::Missing)));
    }

    #[test]
    fn read_condarc_invalid_utf8_returns_invalid_data_unreadable_error() {
        // Given
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(&[0xff, 0xfe, 0xfd]).unwrap();

        // When
        let result = read_condarc(file.path());

        // Then
        match result {
            Err(ReadOutcome::Unreadable(error)) => {
                assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            }
            _ => panic!("expected invalid-data unreadable error"),
        }
    }

    #[test]
    fn read_condarc_valid_populated_file_returns_contents() {
        // Given
        let contents = "channels: [conda-forge]\n";
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(contents.as_bytes()).unwrap();

        // When
        let result = read_condarc(file.path());

        // Then
        assert_eq!(result.ok().as_deref(), Some(contents));
    }
}
