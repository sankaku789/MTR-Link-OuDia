//! OuDia の元バイト列を保持したまま参照するための読込モデル。

use crate::{DomainError, OudiaDirection, SourceRange};

/// 読み込んだ本文の文字コード。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    /// UTF-8。
    Utf8,
    /// Windows-31J (CP932)。
    Windows31J,
}

/// 入力で使用された改行コード。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    /// LF。
    Lf,
    /// CRLF。
    CrLf,
}

/// OuDia の原本と読込結果。
#[derive(Debug, Clone)]
pub struct OudiaSource {
    /// 入力されたままのバイト列。
    pub bytes: Vec<u8>,
    /// UTF-8 BOM の有無。
    pub bom: bool,
    /// 検出した文字コード。
    pub encoding: TextEncoding,
    /// 検出した改行コード。
    pub line_ending: LineEnding,
    /// 構造を参照するための読込モデル。
    pub document: OudiaDocument,
}

impl OudiaSource {
    /// 原本と同じ文字コードで、新規patch文字列だけを符号化する。
    pub fn encode_text(&self, text: &str) -> Result<Vec<u8>, DomainError> {
        match self.encoding {
            TextEncoding::Utf8 => Ok(text.as_bytes().to_vec()),
            TextEncoding::Windows31J => {
                let (bytes, _, errors) = encoding_rs::SHIFT_JIS.encode(text);
                if errors {
                    return Err(DomainError::InvalidValue {
                        value_name: "Windows-31J patch text",
                    });
                }
                Ok(bytes.into_owned())
            }
        }
    }
    /// 無変更保存で使用する原本バイト列を返す。
    pub fn unchanged_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// OuDia 文書の参照モデル。
#[derive(Debug, Clone)]
pub struct OudiaDocument {
    /// FileType の値。
    pub file_type: String,
    /// 基準ダイヤの選択状態。
    pub kijun_dia_index: KijunDiaIndex,
    /// 出現順のプロパティ。
    pub properties: Vec<OudiaProperty>,
    /// 出現順のセクション。
    pub sections: Vec<OudiaSection>,
    /// 出現順の未知行。
    pub unknown_lines: Vec<OudiaRawLine>,
    /// ダイヤとその列車。
    pub diagrams: Vec<OudiaDiagram>,
    /// 出現順の駅スロット名。
    pub station_slots: Vec<OudiaStationSlot>,
}

/// OuDia 路線定義内の駅スロット。
#[derive(Debug, Clone)]
pub struct OudiaStationSlot {
    pub name: String,
    pub name_range: SourceRange,
}

/// 基準ダイヤ指定の読込状態。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KijunDiaIndex {
    /// 指定がないため、利用者の選択が必要である。
    Missing,
    /// 数値として解釈できないため、利用者の選択が必要である。
    Invalid,
    /// 数値だがダイヤ数の範囲外である。
    OutOfRange { index: usize },
    /// 有効なダイヤインデックス。
    Valid(usize),
}

/// `key=value` の元バイト範囲付きプロパティ。
#[derive(Debug, Clone)]
pub struct OudiaProperty {
    pub key: String,
    pub value: String,
    pub whole_line_range: SourceRange,
    pub value_range: SourceRange,
}

/// 未知行の元バイト範囲付き表現。
#[derive(Debug, Clone)]
pub struct OudiaRawLine {
    pub raw: String,
    pub range: SourceRange,
}

/// セクションの元バイト範囲付き表現。
#[derive(Debug, Clone)]
pub struct OudiaSection {
    pub name: String,
    pub header_range: SourceRange,
    pub range: SourceRange,
}

/// ダイヤとその列車。
#[derive(Debug, Clone)]
pub struct OudiaDiagram {
    pub section_range: SourceRange,
    pub trains: Vec<OudiaTrain>,
}

/// 方向付き列車。
#[derive(Debug, Clone)]
pub struct OudiaTrain {
    pub train_number: Option<String>,
    pub direction: OudiaDirection,
    pub section_range: SourceRange,
    /// `Ressyasyubetsu` が数値として取得できた場合の種別インデックス。
    pub train_type_index: Option<usize>,
    pub eki_jikoku: EkiJikoku,
}

/// `EkiJikoku` のセル列。
#[derive(Debug, Clone)]
pub struct EkiJikoku {
    pub cells: Vec<EkiJikokuCell>,
    pub value_range: SourceRange,
}

/// OuDia 時刻の基本形式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OudiaTime {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

/// `EkiJikoku` の1セル。
#[derive(Debug, Clone)]
pub struct EkiJikokuCell {
    pub raw: String,
    pub handling_code: Option<u8>,
    pub arrival: Option<OudiaTime>,
    pub departure: Option<OudiaTime>,
    pub track_index: Option<u32>,
    pub unknown_parts: Vec<String>,
    pub source_range: SourceRange,
}

impl EkiJikokuCell {
    /// 空欄セルかを返す。
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    /// 時刻の有無にかかわらず、列車が通る駅として扱うセルかを返す。
    pub fn is_route_active(&self) -> bool {
        !self.is_empty()
            && (self.handling_code.is_some() || self.arrival.is_some() || self.departure.is_some())
    }

    /// MTRの停車時刻を対応させるセルかを返す。通過セルは経路表示だけに使用する。
    pub fn is_timetable_active(&self) -> bool {
        self.is_route_active() && self.handling_code != Some(2)
    }
}

/// 元バイト列を filesystem 非依存で読み込む。
pub fn parse_oudia(bytes: Vec<u8>) -> Result<OudiaSource, DomainError> {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        return Err(DomainError::UnsupportedEncoding { encoding: "UTF-16" });
    }

    let (bom, body_start, encoding) = if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        decode(&bytes[3..], TextEncoding::Utf8)?;
        (true, 3, TextEncoding::Utf8)
    } else if std::str::from_utf8(&bytes).is_ok() {
        (false, 0, TextEncoding::Utf8)
    } else {
        decode(&bytes, TextEncoding::Windows31J)?;
        (false, 0, TextEncoding::Windows31J)
    };
    let line_ending = detect_line_ending(&bytes[body_start..])?;
    let lines = split_lines(&bytes, body_start, line_ending);
    let mut document = OudiaDocument {
        file_type: String::new(),
        kijun_dia_index: KijunDiaIndex::Missing,
        properties: Vec::new(),
        sections: Vec::new(),
        unknown_lines: Vec::new(),
        diagrams: Vec::new(),
        station_slots: Vec::new(),
    };
    let mut sections: Vec<OpenSection> = Vec::new();
    let mut current_diagram: Option<usize> = None;
    let mut current_direction: Option<OudiaDirection> = None;
    let mut current_train: Option<usize> = None;
    let mut current_station: Option<usize> = None;
    let mut kijun_value = None;

    for line in lines {
        let text = decode(&bytes[line.start..line.end], encoding)?;
        if text.is_empty() {
            continue;
        }
        if text == "." {
            let Some(open) = sections.pop() else {
                return Err(DomainError::InvalidOudiaStructure {
                    reason: "対応する開始セクションがありません",
                });
            };
            document.sections[open.section_index].range = SourceRange::new(open.start, line.end)?;
            match open.name.as_str() {
                "Ressya" => {
                    if let (Some(diagram), Some(train)) = (current_diagram, current_train) {
                        document.diagrams[diagram].trains[train].section_range =
                            SourceRange::new(open.start, line.end)?;
                    }
                    current_train = None;
                }
                "Eki" => current_station = None,
                "Kudari" | "Nobori" => current_direction = None,
                "Dia" => current_diagram = None,
                _ => {}
            }
            continue;
        }
        if let Some(name) = text.strip_suffix('.') {
            if name.is_empty() || name.contains('=') {
                return Err(DomainError::InvalidOudiaStructure {
                    reason: "不正なセクション開始行です",
                });
            }
            let section_index = document.sections.len();
            document.sections.push(OudiaSection {
                name: name.to_owned(),
                header_range: SourceRange::new(line.start, line.end)?,
                range: SourceRange::new(line.start, line.end)?,
            });
            sections.push(OpenSection {
                name: name.to_owned(),
                start: line.start,
                section_index,
            });
            match name {
                "Dia" => {
                    document.diagrams.push(OudiaDiagram {
                        section_range: SourceRange::new(line.start, line.end)?,
                        trains: Vec::new(),
                    });
                    current_diagram = Some(document.diagrams.len() - 1);
                }
                "Kudari" => current_direction = Some(OudiaDirection::Kudari),
                "Nobori" => current_direction = Some(OudiaDirection::Nobori),
                "Ressya" => {
                    let (Some(diagram), Some(direction)) = (current_diagram, current_direction)
                    else {
                        return Err(DomainError::InvalidOudiaStructure {
                            reason: "Ressya が Dia と方向の外側にあります",
                        });
                    };
                    document.diagrams[diagram].trains.push(OudiaTrain {
                        train_number: None,
                        direction,
                        section_range: SourceRange::new(line.start, line.end)?,
                        train_type_index: None,
                        eki_jikoku: EkiJikoku {
                            cells: Vec::new(),
                            value_range: SourceRange::new(line.end, line.end)?,
                        },
                    });
                    current_train = Some(document.diagrams[diagram].trains.len() - 1);
                }
                "Eki" => {
                    document.station_slots.push(OudiaStationSlot {
                        name: String::new(),
                        name_range: SourceRange::new(line.end, line.end)?,
                    });
                    current_station = Some(document.station_slots.len() - 1);
                }
                _ => {}
            }
            continue;
        }
        if let Some(equals) = bytes[line.start..line.end]
            .iter()
            .position(|byte| *byte == b'=')
        {
            let key_end = line.start + equals;
            let value_start = key_end + 1;
            let key = decode(&bytes[line.start..key_end], encoding)?;
            let value = decode(&bytes[value_start..line.end], encoding)?;
            if key.is_empty() {
                return Err(DomainError::InvalidOudiaStructure {
                    reason: "空のプロパティ名です",
                });
            }
            let property = OudiaProperty {
                key,
                value,
                whole_line_range: SourceRange::new(line.start, line.end)?,
                value_range: SourceRange::new(value_start, line.end)?,
            };
            if property.key == "FileType" {
                document.file_type = property.value.clone();
            }
            if property.key == "KijunDiaIndex" {
                kijun_value = Some(property.value.clone());
            }
            if property.key == "EkiJikoku" {
                let (Some(diagram), Some(train)) = (current_diagram, current_train) else {
                    return Err(DomainError::InvalidOudiaStructure {
                        reason: "EkiJikoku が Ressya の外側にあります",
                    });
                };
                document.diagrams[diagram].trains[train].eki_jikoku =
                    parse_eki_jikoku(&bytes, property.value_range, encoding)?;
            }
            if matches!(property.key.as_str(), "Ressyasyubetsu" | "Syubetsu")
                && let (Some(diagram), Some(train)) = (current_diagram, current_train)
            {
                document.diagrams[diagram].trains[train].train_type_index =
                    property.value.parse().ok();
            }
            if property.key == "Ekimei"
                && let Some(station) = current_station
            {
                document.station_slots[station].name = property.value.clone();
                document.station_slots[station].name_range = property.value_range;
            }
            if property.key == "Ressyabangou"
                && let (Some(diagram), Some(train)) = (current_diagram, current_train)
            {
                document.diagrams[diagram].trains[train].train_number =
                    Some(property.value.clone());
            }
            document.properties.push(property);
            continue;
        }
        document.unknown_lines.push(OudiaRawLine {
            raw: text,
            range: SourceRange::new(line.start, line.end)?,
        });
    }
    if !sections.is_empty() {
        return Err(DomainError::InvalidOudiaStructure {
            reason: "閉じられていないセクションがあります",
        });
    }
    if document.file_type != "OuDiaSecond.1.16" && document.file_type != "OuDiaSecond.1.17" {
        return Err(DomainError::UnsupportedFileType {
            file_type: document.file_type,
        });
    }
    document.kijun_dia_index = match kijun_value {
        None => KijunDiaIndex::Missing,
        Some(value) => match value.parse::<usize>() {
            Ok(index) if index < document.diagrams.len() => KijunDiaIndex::Valid(index),
            Ok(index) => KijunDiaIndex::OutOfRange { index },
            Err(_) => KijunDiaIndex::Invalid,
        },
    };

    Ok(OudiaSource {
        bytes,
        bom,
        encoding,
        line_ending,
        document,
    })
}

struct OpenSection {
    name: String,
    start: usize,
    section_index: usize,
}

struct Line {
    start: usize,
    end: usize,
}

fn decode(bytes: &[u8], encoding: TextEncoding) -> Result<String, DomainError> {
    match encoding {
        TextEncoding::Utf8 => std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| DomainError::DecodeError { encoding: "UTF-8" }),
        TextEncoding::Windows31J => encoding_rs::SHIFT_JIS
            .decode_without_bom_handling_and_without_replacement(bytes)
            .map(|value| value.into_owned())
            .ok_or(DomainError::DecodeError {
                encoding: "Windows-31J",
            }),
    }
}

fn detect_line_ending(bytes: &[u8]) -> Result<LineEnding, DomainError> {
    let mut lf = false;
    let mut crlf = false;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\n' if index > 0 && bytes[index - 1] == b'\r' => crlf = true,
            b'\n' => lf = true,
            b'\r' if bytes.get(index + 1) != Some(&b'\n') => {
                return Err(DomainError::MixedLineEnding);
            }
            _ => {}
        }
        index += 1;
    }
    if lf && crlf {
        return Err(DomainError::MixedLineEnding);
    }
    Ok(if crlf {
        LineEnding::CrLf
    } else {
        LineEnding::Lf
    })
}

fn split_lines(bytes: &[u8], body_start: usize, line_ending: LineEnding) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut start = body_start;
    for (index, byte) in bytes.iter().enumerate().skip(body_start) {
        if *byte == b'\n' {
            let end = if line_ending == LineEnding::CrLf {
                index - 1
            } else {
                index
            };
            lines.push(Line { start, end });
            start = index + 1;
        }
    }
    if start < bytes.len() {
        lines.push(Line {
            start,
            end: bytes.len(),
        });
    }
    lines
}

fn parse_eki_jikoku(
    bytes: &[u8],
    value_range: SourceRange,
    encoding: TextEncoding,
) -> Result<EkiJikoku, DomainError> {
    let mut cells = Vec::new();
    let value = &bytes[value_range.start()..value_range.end()];
    let mut start = 0;
    for end in value
        .iter()
        .enumerate()
        .filter_map(|(index, byte)| (*byte == b',').then_some(index))
        .chain(std::iter::once(value.len()))
    {
        let raw_bytes = &value[start..end];
        let raw = decode(raw_bytes, encoding)?;
        let source_range =
            SourceRange::new(value_range.start() + start, value_range.start() + end)?;
        cells.push(parse_cell(raw, source_range)?);
        start = end + 1;
    }
    Ok(EkiJikoku { cells, value_range })
}

fn parse_cell(raw: String, source_range: SourceRange) -> Result<EkiJikokuCell, DomainError> {
    if raw.is_empty() {
        return Ok(EkiJikokuCell {
            raw,
            handling_code: None,
            arrival: None,
            departure: None,
            track_index: None,
            unknown_parts: Vec::new(),
            source_range,
        });
    }
    let (main, track_index) = match raw.split_once('$') {
        Some((main, track)) if !track.is_empty() && !track.contains('$') => (
            main,
            Some(
                track
                    .parse()
                    .map_err(|_| DomainError::UnknownEkiJikoku { raw: raw.clone() })?,
            ),
        ),
        Some(_) => return Err(DomainError::UnknownEkiJikoku { raw }),
        None => (raw.as_str(), None),
    };
    let main = main.to_owned();
    let (handling, times) = match main.split_once(';') {
        Some((handling, times)) if !times.contains(';') => (handling, Some(times)),
        Some(_) => return Err(DomainError::UnknownEkiJikoku { raw }),
        None => (main.as_str(), None),
    };
    let handling_code = if handling.is_empty() {
        None
    } else {
        Some(
            handling
                .parse()
                .map_err(|_| DomainError::UnknownEkiJikoku { raw: raw.clone() })?,
        )
    };
    let (arrival, departure) = match times {
        Some(times) => match times.split_once('/') {
            Some((arrival, departure)) if !departure.contains('/') => {
                (parse_time(arrival)?, parse_time(departure)?)
            }
            Some(_) => return Err(DomainError::UnknownEkiJikoku { raw }),
            None => (None, parse_time(times)?),
        },
        None => (None, None),
    };
    Ok(EkiJikokuCell {
        raw,
        handling_code,
        arrival,
        departure,
        track_index,
        unknown_parts: Vec::new(),
        source_range,
    })
}

pub(crate) fn parse_time(value: &str) -> Result<Option<OudiaTime>, DomainError> {
    if value.is_empty() {
        return Ok(None);
    }
    let (hour, minute, second) = if value.contains(':') {
        let mut parts = value.split(':');
        let (Some(hour), Some(minute), Some(second), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(DomainError::UnknownEkiJikoku {
                raw: value.to_owned(),
            });
        };
        (hour, minute, second)
    } else if value.bytes().all(|byte| byte.is_ascii_digit()) {
        match value.len() {
            3 | 4 => (&value[..value.len() - 2], &value[value.len() - 2..], "0"),
            5 | 6 => (
                &value[..value.len() - 4],
                &value[value.len() - 4..value.len() - 2],
                &value[value.len() - 2..],
            ),
            _ => {
                return Err(DomainError::UnknownEkiJikoku {
                    raw: value.to_owned(),
                });
            }
        }
    } else {
        return Err(DomainError::UnknownEkiJikoku {
            raw: value.to_owned(),
        });
    };
    let parsed = OudiaTime {
        hour: hour.parse().map_err(|_| DomainError::UnknownEkiJikoku {
            raw: value.to_owned(),
        })?,
        minute: minute.parse().map_err(|_| DomainError::UnknownEkiJikoku {
            raw: value.to_owned(),
        })?,
        second: second.parse().map_err(|_| DomainError::UnknownEkiJikoku {
            raw: value.to_owned(),
        })?,
    };
    if parsed.minute >= 60 || parsed.second >= 60 {
        return Err(DomainError::UnknownEkiJikoku {
            raw: value.to_owned(),
        });
    }
    Ok(Some(parsed))
}
