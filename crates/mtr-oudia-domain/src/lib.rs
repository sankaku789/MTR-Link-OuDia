//! 外部システムに依存しない業務規則を置く Domain 層。

/// Domain 層を表す最小の型。
///
/// P01 では層の依存境界だけを定義し、業務モデルは後続フェーズで追加する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DomainLayer;
