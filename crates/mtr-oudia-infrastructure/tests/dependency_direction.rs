use mtr_oudia_application::DomainPort;
use mtr_oudia_domain::DomainLayer;
use mtr_oudia_infrastructure::StaticDomainPort;

#[test]
fn infrastructure_implements_the_application_port_with_a_domain_type() {
    let port = StaticDomainPort;

    assert_eq!(port.domain_layer(), DomainLayer);
}
