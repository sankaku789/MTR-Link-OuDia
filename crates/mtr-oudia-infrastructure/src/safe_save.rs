//! 原本を上書きせず、検証済みの OuDia 出力だけを確定する保存器。

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use mtr_oudia_domain::{OudiaPatch, OudiaPatchError, parse_oudia};
use sha2::{Digest, Sha256};

/// 読込時の原本識別に使う SHA-256。
pub type InputHash = [u8; 32];

/// filesystem 保存を成功として扱えない理由。
#[derive(Debug)]
pub enum SafeSaveError {
    Io(io::Error),
    InputChanged,
    SamePath,
    OutputAlreadyExists,
    Patch(OudiaPatchError),
    Reparse(mtr_oudia_domain::DomainError),
    FinalizeRace,
}

impl std::fmt::Display for SafeSaveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "filesystem error: {error}"),
            Self::InputChanged => formatter.write_str("入力ファイルが外部変更されました"),
            Self::SamePath => formatter.write_str("入力と出力が同じファイルです"),
            Self::OutputAlreadyExists => formatter.write_str("出力先は既に存在します"),
            Self::Patch(error) => write!(formatter, "パッチ検証失敗: {error}"),
            Self::Reparse(error) => write!(formatter, "保存後の再解析失敗: {error}"),
            Self::FinalizeRace => formatter.write_str("出力先が確定中に作成されました"),
        }
    }
}

impl std::error::Error for SafeSaveError {}

/// P09 が検証を迂回せずに呼び出す安全保存入口。
#[derive(Debug, Default)]
pub struct SafeOudiaWriter;

impl SafeOudiaWriter {
    pub const fn new() -> Self {
        Self
    }

    pub fn hash(&self, bytes: &[u8]) -> InputHash {
        Sha256::digest(bytes).into()
    }

    /// 入力を一度だけ読み、同じバイト列だけをパッチ元として出力を確定する。
    pub fn save(
        &self,
        input: impl AsRef<Path>,
        output: impl AsRef<Path>,
        expected_hash: InputHash,
        patch: &OudiaPatch,
    ) -> Result<(), SafeSaveError> {
        let input = input.as_ref();
        let output = output.as_ref();
        if same_path(input, output)? {
            return Err(SafeSaveError::SamePath);
        }
        if output.exists() {
            return Err(SafeSaveError::OutputAlreadyExists);
        }

        // この一回の read の結果以外をパッチ元にしない。
        let original = fs::read(input).map_err(SafeSaveError::Io)?;
        if self.hash(&original) != expected_hash {
            return Err(SafeSaveError::InputChanged);
        }
        let saved = patch.apply(&original).map_err(SafeSaveError::Patch)?;
        parse_oudia(saved.clone()).map_err(SafeSaveError::Reparse)?;

        let parent = output.parent().ok_or_else(|| {
            SafeSaveError::Io(io::Error::other("出力先親ディレクトリがありません"))
        })?;
        let temporary = create_temp(parent)?;
        let result = write_and_finalize(&temporary, output, &saved);
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

fn same_path(input: &Path, output: &Path) -> Result<bool, SafeSaveError> {
    let input = fs::canonicalize(input).map_err(SafeSaveError::Io)?;
    let output = if output.exists() {
        fs::canonicalize(output).map_err(SafeSaveError::Io)?
    } else {
        let parent = output.parent().unwrap_or_else(|| Path::new("."));
        fs::canonicalize(parent)
            .map_err(SafeSaveError::Io)?
            .join(output.file_name().unwrap_or_default())
    };
    Ok(input == output)
}

fn create_temp(parent: &Path) -> Result<PathBuf, SafeSaveError> {
    for nonce in 0..128_u32 {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| SafeSaveError::Io(io::Error::other(error)))?
            .as_nanos();
        let path = parent.join(format!(".mtr-oudia-{nanos}-{nonce}.tmp"));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(SafeSaveError::Io(error)),
        }
    }
    Err(SafeSaveError::Io(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "一時ファイル名を確保できません",
    )))
}

fn write_and_finalize(temporary: &Path, output: &Path, bytes: &[u8]) -> Result<(), SafeSaveError> {
    let mut file = OpenOptions::new()
        .write(true)
        .open(temporary)
        .map_err(SafeSaveError::Io)?;
    file.write_all(bytes).map_err(SafeSaveError::Io)?;
    file.flush().map_err(SafeSaveError::Io)?;
    file.sync_all().map_err(SafeSaveError::Io)?;
    drop(file);
    // hard link は既存出力を置換しないので rename の OS 差を避けられる。
    fs::hard_link(temporary, output).map_err(|error| {
        if error.kind() == io::ErrorKind::AlreadyExists {
            SafeSaveError::FinalizeRace
        } else {
            SafeSaveError::Io(error)
        }
    })?;
    let _ = File::open(output.parent().unwrap_or_else(|| Path::new(".")))
        .and_then(|directory| directory.sync_all());
    // 確定済み出力を失敗扱いにしないため、後始末失敗は孤立tempとして扱う。
    let _ = fs::remove_file(temporary);
    Ok(())
}
