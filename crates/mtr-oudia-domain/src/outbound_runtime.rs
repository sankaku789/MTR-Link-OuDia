use crate::{DomainError, ServiceTimeMillis};

const MILLIS_PER_DAY: i64 = 86_400_000;

/// 車庫発から始発駅着までの非負duration。始発駅の停車時間は含まない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutboundRuntime(i64);

impl OutboundRuntime {
    pub fn new(millis: i64) -> Result<Self, DomainError> {
        if millis < 0 {
            return Err(DomainError::InvalidValue {
                value_name: "outbound runtime",
            });
        }
        Ok(Self(millis))
    }

    pub fn from_seconds(seconds: i64) -> Result<Self, DomainError> {
        Self::new(
            seconds
                .checked_mul(1_000)
                .ok_or(DomainError::TimeOverflow)?,
        )
    }

    pub const fn millis(self) -> i64 {
        self.0
    }

    /// 同じ時間領域の日内時刻間の、前向きの日跨ぎ差分。
    pub fn between_daily_times(
        depot_departure: ServiceTimeMillis,
        first_arrival: ServiceTimeMillis,
    ) -> Result<Self, DomainError> {
        validate_daily_time(depot_departure)?;
        validate_daily_time(first_arrival)?;
        Self::new((first_arrival.millis() - depot_departure.millis()).rem_euclid(MILLIS_PER_DAY))
    }

    /// 始発駅着の日内時刻から出区の日内時刻を逆算する。
    pub fn outbound_time(
        self,
        first_arrival: ServiceTimeMillis,
    ) -> Result<ServiceTimeMillis, DomainError> {
        validate_daily_time(first_arrival)?;
        ServiceTimeMillis::new(
            (first_arrival.millis() - self.0.rem_euclid(MILLIS_PER_DAY)).rem_euclid(MILLIS_PER_DAY),
        )
    }

    /// 始発駅発しかない場合、停車時間を除いて到着基準へ戻す。
    pub fn outbound_time_from_departure(
        self,
        first_departure: ServiceTimeMillis,
        first_dwell: ServiceTimeMillis,
    ) -> Result<ServiceTimeMillis, DomainError> {
        validate_daily_time(first_departure)?;
        let arrival = ServiceTimeMillis::new(
            (first_departure.millis() - first_dwell.millis().rem_euclid(MILLIS_PER_DAY))
                .rem_euclid(MILLIS_PER_DAY),
        )?;
        self.outbound_time(arrival)
    }
}

fn validate_daily_time(time: ServiceTimeMillis) -> Result<(), DomainError> {
    if time.millis() >= MILLIS_PER_DAY {
        Err(DomainError::InvalidValue {
            value_name: "daily clock time",
        })
    } else {
        Ok(())
    }
}
