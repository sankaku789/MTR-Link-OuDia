//! 元バイト列を変更範囲だけで更新するためのパッチ。

use crate::{GeneratedTimetable, OudiaRouteTemplate, OudiaSource, SourceRange};
use std::error::Error;
use std::fmt;

/// 置換の用途。保存後検証で許可された差分を識別する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteReplacementKind {
    Time,
    Operation,
}

/// 元入力バイト列に対する単一の置換。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteReplacement {
    pub range: SourceRange,
    pub expected: Vec<u8>,
    pub replacement: Vec<u8>,
    pub kind: ByteReplacementKind,
}

/// 検証済みの OuDia バイトパッチ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OudiaPatch {
    replacements: Vec<ByteReplacement>,
}

impl OudiaPatch {
    /// 重複しない元入力範囲だけからパッチを作る。
    pub fn new(mut replacements: Vec<ByteReplacement>) -> Result<Self, OudiaPatchError> {
        replacements.sort_by_key(|replacement| replacement.range.start());
        for pair in replacements.windows(2) {
            if pair[0].range.end() > pair[1].range.start() {
                return Err(OudiaPatchError::OverlappingRanges {
                    left: pair[0].range,
                    right: pair[1].range,
                });
            }
        }
        Ok(Self { replacements })
    }

    /// 置換対象を元入力基準の昇順で返す。
    pub fn replacements(&self) -> &[ByteReplacement] {
        &self.replacements
    }

    /// 元入力を一度だけ走査して、長さが変化する置換も安全に適用する。
    pub fn apply(&self, original: &[u8]) -> Result<Vec<u8>, OudiaPatchError> {
        let mut result = Vec::with_capacity(original.len());
        let mut cursor = 0;
        for replacement in &self.replacements {
            let range = replacement.range;
            if range.end() > original.len() {
                return Err(OudiaPatchError::RangeOutOfBounds {
                    range,
                    bounds: original.len(),
                });
            }
            if original[range.start()..range.end()] != replacement.expected {
                return Err(OudiaPatchError::ExpectedBytesMismatch { range });
            }
            result.extend_from_slice(&original[cursor..range.start()]);
            result.extend_from_slice(&replacement.replacement);
            cursor = range.end();
        }
        result.extend_from_slice(&original[cursor..]);
        Ok(result)
    }
}

/// パッチ生成または適用を安全に完了できない理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OudiaPatchError {
    OverlappingRanges {
        left: SourceRange,
        right: SourceRange,
    },
    RangeOutOfBounds {
        range: SourceRange,
        bounds: usize,
    },
    ExpectedBytesMismatch {
        range: SourceRange,
    },
}

impl fmt::Display for OudiaPatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OverlappingRanges { .. } => formatter.write_str("パッチ範囲が重複しています"),
            Self::RangeOutOfBounds { .. } => formatter.write_str("パッチ範囲が入力外です"),
            Self::ExpectedBytesMismatch { .. } => {
                formatter.write_str("パッチ対象が読込時の内容と一致しません")
            }
        }
    }
}

impl Error for OudiaPatchError {}

/// 対象列車に属する Operation 行の扱い。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OperationPolicy {
    /// Operation があるのに利用者の確認が未完了である。
    Unselected,
    /// Operation の生バイト列を変更しない。意味整合性は保証しない。
    #[default]
    Preserve,
    /// 対象列車内で一意に特定できる Operation 行だけを削除する。
    RemoveTargetTrain,
}

/// 時刻パッチ計画を安全に生成できない理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EkiJikokuPatchError {
    TargetNotFound,
    CellStructureMismatch,
    EmptyCell,
    TimeShapeMismatch,
    UnsupportedOvernightTime,
    AmbiguousOperation,
    InvalidPatch(OudiaPatchError),
}

impl fmt::Display for EkiJikokuPatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TargetNotFound => formatter.write_str("対象列車が原本内に見つかりません"),
            Self::CellStructureMismatch => {
                formatter.write_str("対象列車の駅セル構造がプレビューと一致しません")
            }
            Self::EmptyCell => formatter.write_str("更新対象に空の駅セルがあります"),
            Self::TimeShapeMismatch => {
                formatter.write_str("既存セルと生成時刻の着発形式が一致しません")
            }
            Self::UnsupportedOvernightTime => {
                formatter.write_str("24時を超える時刻は保存できません")
            }
            Self::AmbiguousOperation => formatter.write_str("Operation行を一意に処理できません"),
            Self::InvalidPatch(error) => write!(formatter, "パッチ検証失敗: {error}"),
        }
    }
}

impl Error for EkiJikokuPatchError {}

/// テンプレートの active cell だけを、既存の時刻文字列範囲で置換する。
pub fn build_eki_jikoku_patch(
    source: &OudiaSource,
    template: &OudiaRouteTemplate,
    timetable: &GeneratedTimetable,
    operation_policy: OperationPolicy,
) -> Result<OudiaPatch, EkiJikokuPatchError> {
    if timetable.crosses_midnight
        || timetable.stops.iter().any(|stop| {
            stop.rounded_arrival_seconds
                .is_some_and(|seconds| seconds >= 86_400)
                || stop
                    .rounded_departure_seconds
                    .is_some_and(|seconds| seconds >= 86_400)
        })
    {
        return Err(EkiJikokuPatchError::UnsupportedOvernightTime);
    }
    let diagram = source
        .document
        .diagrams
        .get(template.diagram_index)
        .ok_or(EkiJikokuPatchError::TargetNotFound)?;
    let train = diagram
        .trains
        .get(template.train_index)
        .ok_or(EkiJikokuPatchError::TargetNotFound)?;
    if train.direction != template.direction || train.section_range != template.source_train_range {
        return Err(EkiJikokuPatchError::TargetNotFound);
    }
    let active = train
        .eki_jikoku
        .cells
        .iter()
        .enumerate()
        .filter(|(_, cell)| {
            !cell.is_empty() && (cell.arrival.is_some() || cell.departure.is_some())
        })
        .collect::<Vec<_>>();
    if active.iter().map(|(index, _)| *index).collect::<Vec<_>>() != template.active_station_slots
        || active.len() != timetable.stops.len()
    {
        return Err(EkiJikokuPatchError::CellStructureMismatch);
    }

    let mut replacements = Vec::new();
    for ((_, cell), stop) in active.into_iter().zip(&timetable.stops) {
        if cell.is_empty() {
            return Err(EkiJikokuPatchError::EmptyCell);
        }
        let (arrival_range, departure_range) = time_ranges(&source.bytes, cell.source_range)
            .ok_or(EkiJikokuPatchError::CellStructureMismatch)?;
        add_time_replacement(
            &mut replacements,
            arrival_range,
            cell.arrival.is_some(),
            stop.rounded_arrival_display.as_deref(),
            &source.bytes,
        )?;
        add_time_replacement(
            &mut replacements,
            departure_range,
            cell.departure.is_some(),
            stop.rounded_departure_display.as_deref(),
            &source.bytes,
        )?;
    }
    let operations = source
        .document
        .properties
        .iter()
        .filter(|property| {
            property.key.starts_with("Operation")
                && property.whole_line_range.start() >= train.section_range.start()
                && property.whole_line_range.end() <= train.section_range.end()
        })
        .collect::<Vec<_>>();
    if operation_policy == OperationPolicy::Unselected && !operations.is_empty() {
        return Err(EkiJikokuPatchError::AmbiguousOperation);
    }
    if operation_policy == OperationPolicy::RemoveTargetTrain {
        if operations.len() != 1 {
            return Err(EkiJikokuPatchError::AmbiguousOperation);
        }
        let property = operations[0];
        replacements.push(ByteReplacement {
            range: property.whole_line_range,
            expected: source.bytes
                [property.whole_line_range.start()..property.whole_line_range.end()]
                .to_vec(),
            replacement: Vec::new(),
            kind: ByteReplacementKind::Operation,
        });
    }
    OudiaPatch::new(replacements).map_err(EkiJikokuPatchError::InvalidPatch)
}

fn add_time_replacement(
    replacements: &mut Vec<ByteReplacement>,
    range: SourceRange,
    exists: bool,
    value: Option<&str>,
    bytes: &[u8],
) -> Result<(), EkiJikokuPatchError> {
    if exists != value.is_some() {
        return Err(EkiJikokuPatchError::TimeShapeMismatch);
    }
    if let Some(value) = value {
        let expected = &bytes[range.start()..range.end()];
        replacements.push(ByteReplacement {
            range,
            expected: expected.to_vec(),
            replacement: format_time_like_original(value, expected)?,
            kind: ByteReplacementKind::Time,
        });
    }
    Ok(())
}

fn time_ranges(bytes: &[u8], cell: SourceRange) -> Option<(SourceRange, SourceRange)> {
    let raw = &bytes[cell.start()..cell.end()];
    let semicolon = raw.iter().position(|byte| *byte == b';')?;
    let time_end = raw
        .iter()
        .position(|byte| *byte == b'$')
        .unwrap_or(raw.len());
    let time_start = semicolon + 1;
    if time_start > time_end {
        return None;
    }
    if let Some(slash) = raw[time_start..time_end]
        .iter()
        .position(|byte| *byte == b'/')
        .map(|position| time_start + position)
    {
        Some((
            SourceRange::new(cell.start() + time_start, cell.start() + slash).ok()?,
            SourceRange::new(cell.start() + slash + 1, cell.start() + time_end).ok()?,
        ))
    } else {
        Some((
            SourceRange::new(cell.start() + time_start, cell.start() + time_start).ok()?,
            SourceRange::new(cell.start() + time_start, cell.start() + time_end).ok()?,
        ))
    }
}

fn format_time_like_original(value: &str, original: &[u8]) -> Result<Vec<u8>, EkiJikokuPatchError> {
    if original.contains(&b':') {
        return Ok(value.as_bytes().to_vec());
    }
    let mut parts = value.split(':');
    let (Some(hour), Some(minute), Some(second), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(EkiJikokuPatchError::TimeShapeMismatch);
    };
    let hour: u8 = hour
        .parse()
        .map_err(|_| EkiJikokuPatchError::TimeShapeMismatch)?;
    let minute: u8 = minute
        .parse()
        .map_err(|_| EkiJikokuPatchError::TimeShapeMismatch)?;
    let second: u8 = second
        .parse()
        .map_err(|_| EkiJikokuPatchError::TimeShapeMismatch)?;
    let (hour_width, original_has_seconds) = match original.len() {
        3 => (1, false),
        4 => (2, false),
        5 => (1, true),
        6 => (2, true),
        _ => return Err(EkiJikokuPatchError::TimeShapeMismatch),
    };
    let formatted = if original_has_seconds || second != 0 {
        format!("{hour:0hour_width$}{minute:02}{second:02}")
    } else {
        format!("{hour:0hour_width$}{minute:02}")
    };
    Ok(formatted.into_bytes())
}
