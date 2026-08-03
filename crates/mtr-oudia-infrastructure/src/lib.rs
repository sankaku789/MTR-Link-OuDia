//! Application の Port 実装を置く Infrastructure 層。

use mtr_oudia_application::DomainPort;
use mtr_oudia_domain::DomainLayer;

/// P01 の Application Port 実装。
pub struct StaticDomainPort;

impl DomainPort for StaticDomainPort {
    fn domain_layer(&self) -> DomainLayer {
        DomainLayer
    }
}
