//! OuDia の基準列車を経路テンプレートへ変換し、MTR 駅列と照合する処理。

use std::collections::BTreeMap;

use unicode_normalization::UnicodeNormalization;

use crate::{KijunDiaIndex, OudiaDirection, OudiaDocument, SourceRange};

/// EkiJikoku の使用セルに保持する停車状態。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OudiaStopState {
    pub handling_code: Option<u8>,
}

/// 基準列車から抽出した経路テンプレート。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OudiaRouteTemplate {
    pub diagram_index: usize,
    pub direction: OudiaDirection,
    pub train_index: usize,
    pub train_type_index: Option<usize>,
    pub active_station_slots: Vec<usize>,
    pub stop_pattern: Vec<OudiaStopState>,
    pub source_train_range: SourceRange,
    pub eki_jikoku_value_range: SourceRange,
    /// active_station_slots と同じ順序の駅名。スロット番号は失わない。
    pub station_slot_names: Vec<String>,
}

/// 選択済みダイヤから抽出したテンプレート群。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OudiaRouteTemplates {
    pub diagram_index: usize,
    pub templates: Vec<OudiaRouteTemplate>,
}

/// 基準ダイヤが未確定の場合に GUI へ表示する候補。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OudiaDiagramCandidate {
    pub diagram_index: usize,
    pub train_count: usize,
}

/// 基準ダイヤの選択結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferenceDiagramSelection {
    Selected(OudiaRouteTemplates),
    NeedsSelection {
        candidates: Vec<OudiaDiagramCandidate>,
    },
}

/// Valid な KijunDiaIndex に限り基準ダイヤのテンプレートを自動抽出する。
pub fn build_oudia_route_templates(document: &OudiaDocument) -> ReferenceDiagramSelection {
    match document.kijun_dia_index {
        KijunDiaIndex::Valid(index) if index < document.diagrams.len() => {
            ReferenceDiagramSelection::Selected(templates_for_diagram(document, index))
        }
        KijunDiaIndex::Missing
        | KijunDiaIndex::Invalid
        | KijunDiaIndex::OutOfRange { .. }
        | KijunDiaIndex::Valid(_) => ReferenceDiagramSelection::NeedsSelection {
            candidates: document
                .diagrams
                .iter()
                .enumerate()
                .map(|(diagram_index, diagram)| OudiaDiagramCandidate {
                    diagram_index,
                    train_count: diagram.trains.len(),
                })
                .collect(),
        },
    }
}

fn templates_for_diagram(document: &OudiaDocument, diagram_index: usize) -> OudiaRouteTemplates {
    let diagram = &document.diagrams[diagram_index];
    let templates = diagram
        .trains
        .iter()
        .enumerate()
        .map(|(train_index, train)| {
            let active = train
                .eki_jikoku
                .cells
                .iter()
                .enumerate()
                .filter(|(_, cell)| {
                    // 時刻を持たない非営業セルは、空欄と同様に経路へ補完しない。
                    !cell.is_empty() && (cell.arrival.is_some() || cell.departure.is_some())
                })
                .collect::<Vec<_>>();
            OudiaRouteTemplate {
                diagram_index,
                direction: train.direction,
                train_index,
                train_type_index: train.train_type_index,
                active_station_slots: active.iter().map(|(index, _)| *index).collect(),
                stop_pattern: active
                    .iter()
                    .map(|(_, cell)| OudiaStopState {
                        handling_code: cell.handling_code,
                    })
                    .collect(),
                source_train_range: train.section_range,
                eki_jikoku_value_range: train.eki_jikoku.value_range,
                station_slot_names: active
                    .iter()
                    .map(|(index, _)| {
                        document
                            .station_slots
                            .get(*index)
                            .map(|slot| slot.name.clone())
                            .unwrap_or_default()
                    })
                    .collect(),
            }
        })
        .collect();
    OudiaRouteTemplates {
        diagram_index,
        templates,
    }
}

/// 手動選択で再利用できる、テンプレート由来の安定 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteCandidateId {
    pub diagram_index: usize,
    pub direction: OudiaDirection,
    pub train_index: usize,
}

/// 推測重みを使わない一致優先順位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RouteMatchRank {
    PrimaryExact,
    MultilingualExact,
    AliasExact,
    CollapsedDuplicateExact,
    Partial,
}

/// MTR 駅と OuDia スロットの対応。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StationMapping {
    pub mtr_station_index: usize,
    pub oudia_slot_index: usize,
}

/// 手動選択・自動確定いずれにも使用する候補詳細。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteMatchCandidate {
    pub id: RouteCandidateId,
    pub direction: OudiaDirection,
    pub rank: RouteMatchRank,
    pub station_mapping: Vec<StationMapping>,
    pub diagnostics: Vec<RouteMatchDiagnostic>,
}

/// 確定しない理由を UI へ渡す診断。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteMatchDiagnostic {
    PartialSequence,
    TrainTypeNotSelected,
    TrainTypeMismatch,
    MultipleCandidates,
    NoCandidate,
}

/// 照合の明示的な結果状態。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteMatchOutcome {
    NoCandidate {
        diagnostics: Vec<RouteMatchDiagnostic>,
    },
    Automatic {
        candidate: RouteMatchCandidate,
    },
    Manual {
        candidates: Vec<RouteMatchCandidate>,
        diagnostics: Vec<RouteMatchDiagnostic>,
    },
}

/// OuDia 駅名の候補を正規化する。case は Unicode 小文字へ統一する方針である。
pub fn normalize_station_name(value: &str) -> String {
    value
        .nfkc()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn station_name_candidates(value: &str) -> Vec<String> {
    value
        .split_once("||")
        .map_or(value, |(name, _)| name)
        .split('|')
        .map(normalize_station_name)
        .filter(|name| !name.is_empty())
        .collect()
}

/// 駅列を照合する。列車種別が明示選択されない限り、完全一致でも自動確定しない。
pub fn match_mtr_route(
    mtr_station_names: &[String],
    templates: &[OudiaRouteTemplate],
    aliases: &BTreeMap<String, String>,
    selected_train_type: Option<usize>,
) -> RouteMatchOutcome {
    let mut candidates = templates
        .iter()
        .filter_map(|template| candidate_for(mtr_station_names, template, aliases))
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return RouteMatchOutcome::NoCandidate {
            diagnostics: vec![RouteMatchDiagnostic::NoCandidate],
        };
    }
    // 優先順位が低い部分一致は、完全一致候補がある場合に曖昧性へ持ち込まない。
    let best_rank = candidates
        .iter()
        .map(|candidate| candidate.rank)
        .min()
        .unwrap();
    candidates.retain(|candidate| candidate.rank == best_rank);
    let matched_train_type = templates
        .iter()
        .find(|template| {
            template.diagram_index == candidates[0].id.diagram_index
                && template.direction == candidates[0].id.direction
                && template.train_index == candidates[0].id.train_index
        })
        .and_then(|template| template.train_type_index);
    let automatic = candidates.len() == 1
        && candidates[0].rank != RouteMatchRank::Partial
        && matched_train_type.is_some()
        && selected_train_type == matched_train_type;
    if automatic {
        return RouteMatchOutcome::Automatic {
            candidate: candidates.remove(0),
        };
    }
    let mut diagnostics = Vec::new();
    if candidates.len() > 1 {
        diagnostics.push(RouteMatchDiagnostic::MultipleCandidates);
    }
    if selected_train_type.is_none() {
        diagnostics.push(RouteMatchDiagnostic::TrainTypeNotSelected);
    } else if selected_train_type != matched_train_type {
        diagnostics.push(RouteMatchDiagnostic::TrainTypeMismatch);
    }
    RouteMatchOutcome::Manual {
        candidates,
        diagnostics,
    }
}

fn candidate_for(
    mtr: &[String],
    template: &OudiaRouteTemplate,
    aliases: &BTreeMap<String, String>,
) -> Option<RouteMatchCandidate> {
    if mtr.len() != template.station_slot_names.len() || mtr.is_empty() {
        return None;
    }
    let levels = mtr
        .iter()
        .zip(&template.station_slot_names)
        .map(|(left, right)| name_level(left, right, aliases))
        .collect::<Vec<_>>();
    if levels.iter().all(Option::is_some) {
        let rank = levels
            .into_iter()
            .flatten()
            .max()
            .unwrap_or(RouteMatchRank::PrimaryExact);
        return Some(make_candidate(
            template,
            rank,
            (0..mtr.len()).collect(),
            Vec::new(),
        ));
    }

    let mtr_collapsed = collapse_names(mtr);
    let oudia_collapsed = collapse_names(&template.station_slot_names);
    if mtr_collapsed.len() == oudia_collapsed.len()
        && mtr_collapsed
            .iter()
            .zip(&oudia_collapsed)
            .all(|((name, _), (other, _))| name == other)
    {
        let station_mapping = mtr_collapsed
            .iter()
            .zip(&oudia_collapsed)
            .flat_map(|((_, mtr_indices), (_, oudia_indices))| {
                mtr_indices
                    .iter()
                    .map(move |mtr_station_index| StationMapping {
                        mtr_station_index: *mtr_station_index,
                        oudia_slot_index: template.active_station_slots[oudia_indices[0]],
                    })
            })
            .collect();
        return Some(RouteMatchCandidate {
            id: RouteCandidateId {
                diagram_index: template.diagram_index,
                direction: template.direction,
                train_index: template.train_index,
            },
            direction: template.direction,
            rank: RouteMatchRank::CollapsedDuplicateExact,
            station_mapping,
            diagnostics: Vec::new(),
        });
    }

    let matching = levels
        .iter()
        .enumerate()
        .filter_map(|(index, level)| level.is_some().then_some(index))
        .collect::<Vec<_>>();
    if mtr.len() >= 3
        && matching.first() == Some(&0)
        && matching.last() == Some(&(mtr.len() - 1))
        && matching.len() >= 2
    {
        return Some(make_candidate(
            template,
            RouteMatchRank::Partial,
            matching,
            vec![RouteMatchDiagnostic::PartialSequence],
        ));
    }
    None
}

fn collapse_names(names: &[String]) -> Vec<(String, Vec<usize>)> {
    let mut collapsed: Vec<(String, Vec<usize>)> = Vec::new();
    for (index, name) in names.iter().enumerate() {
        let normalized = station_name_candidates(name)
            .into_iter()
            .next()
            .unwrap_or_default();
        if let Some((previous, indices)) = collapsed.last_mut()
            && *previous == normalized
        {
            indices.push(index);
        } else {
            collapsed.push((normalized, vec![index]));
        }
    }
    collapsed
}

fn name_level(
    left: &str,
    right: &str,
    aliases: &BTreeMap<String, String>,
) -> Option<RouteMatchRank> {
    let left_names = station_name_candidates(left);
    let right_names = station_name_candidates(right);
    if left_names.first() == right_names.first() {
        return Some(RouteMatchRank::PrimaryExact);
    }
    if left_names.iter().any(|name| right_names.contains(name)) {
        return Some(RouteMatchRank::MultilingualExact);
    }
    let aliases = aliases
        .iter()
        .map(|(from, to)| (normalize_station_name(from), normalize_station_name(to)))
        .collect::<BTreeMap<_, _>>();
    if left_names
        .iter()
        .map(|name| aliases.get(name).unwrap_or(name))
        .any(|name| {
            right_names
                .iter()
                .map(|candidate| aliases.get(candidate).unwrap_or(candidate))
                .any(|candidate| name == candidate)
        })
    {
        return Some(RouteMatchRank::AliasExact);
    }
    None
}

fn make_candidate(
    template: &OudiaRouteTemplate,
    rank: RouteMatchRank,
    mtr_indices: Vec<usize>,
    diagnostics: Vec<RouteMatchDiagnostic>,
) -> RouteMatchCandidate {
    RouteMatchCandidate {
        id: RouteCandidateId {
            diagram_index: template.diagram_index,
            direction: template.direction,
            train_index: template.train_index,
        },
        direction: template.direction,
        rank,
        station_mapping: mtr_indices
            .into_iter()
            .zip(&template.active_station_slots)
            .map(|(mtr_station_index, oudia_slot_index)| StationMapping {
                mtr_station_index,
                oudia_slot_index: *oudia_slot_index,
            })
            .collect(),
        diagnostics,
    }
}
