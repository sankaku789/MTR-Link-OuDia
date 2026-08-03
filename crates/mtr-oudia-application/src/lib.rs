//! Domain のユースケース境界と Port を置く Application 層。

use mtr_oudia_domain::DomainLayer;

/// Infrastructure が実装する、Domain 型を返す最小の Port。
pub trait DomainPort {
    /// Domain 層を返す。
    fn domain_layer(&self) -> DomainLayer;
}
