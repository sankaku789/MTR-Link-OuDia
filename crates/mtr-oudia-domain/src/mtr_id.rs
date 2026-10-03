use crate::DomainError;

/// MTR Java longのビット列。hex/numericの違いに依存しない識別子。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MtrId(u64);

impl MtrId {
    pub fn from_hex(value: &str) -> Result<Self, DomainError> {
        if value.is_empty() || value.len() > 16 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(DomainError::InvalidValue {
                value_name: "MTR hex ID",
            });
        }
        u64::from_str_radix(value, 16)
            .map(Self)
            .map_err(|_| DomainError::InvalidValue {
                value_name: "MTR hex ID",
            })
    }
    pub const fn from_java_long(value: i64) -> Self {
        Self(value as u64)
    }
    pub fn to_hex(self) -> String {
        format!("{:016X}", self.0)
    }
}
