use crate::errors::TelemetryError;
use crate::timestamp::DurationNanos;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Completeness {
    Complete,
    Partial,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum QualityFlag {
    Estimated,
    InconsistentSourceData,
    InsufficientSamplingInterval,
    ProviderReportedUncertainty,
    KnownStale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DerivationMethod {
    CounterDelta,
    UnitConversion,
    ArithmeticMean,
    Minimum,
    Maximum,
    Percentile50,
    Percentile95,
    Percentile99,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProviderUncertainty {
    absolute_error: u64,
}

impl ProviderUncertainty {
    pub fn new(absolute_error: u64) -> Result<Self, TelemetryError> {
        if absolute_error == 0 {
            return Err(TelemetryError::InvalidSamplingMetadata);
        }
        Ok(Self { absolute_error })
    }

    pub const fn absolute_error(self) -> u64 {
        self.absolute_error
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SamplingMetadata {
    sample_interval: Option<DurationNanos>,
    source_resolution: Option<u64>,
    observation_count: Option<u32>,
    aggregation_window: Option<DurationNanos>,
    missing_sample_count: u32,
    derivation_method: Option<DerivationMethod>,
}

impl SamplingMetadata {
    pub fn new(
        sample_interval: Option<DurationNanos>,
        source_resolution: Option<u64>,
        observation_count: Option<u32>,
        aggregation_window: Option<DurationNanos>,
        missing_sample_count: u32,
        derivation_method: Option<DerivationMethod>,
    ) -> Result<Self, TelemetryError> {
        if sample_interval.is_some_and(DurationNanos::is_zero)
            || aggregation_window.is_some_and(DurationNanos::is_zero)
            || source_resolution == Some(0)
            || observation_count == Some(0)
            || observation_count
                .is_some_and(|count| count.checked_add(missing_sample_count).is_none())
        {
            return Err(TelemetryError::InvalidSamplingMetadata);
        }
        Ok(Self {
            sample_interval,
            source_resolution,
            observation_count,
            aggregation_window,
            missing_sample_count,
            derivation_method,
        })
    }

    pub const fn sample_interval(self) -> Option<DurationNanos> {
        self.sample_interval
    }

    pub const fn source_resolution(self) -> Option<u64> {
        self.source_resolution
    }

    pub const fn observation_count(self) -> Option<u32> {
        self.observation_count
    }

    pub const fn aggregation_window(self) -> Option<DurationNanos> {
        self.aggregation_window
    }

    pub const fn missing_sample_count(self) -> u32 {
        self.missing_sample_count
    }

    pub const fn derivation_method(self) -> Option<DerivationMethod> {
        self.derivation_method
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QualityMetadata {
    completeness: Completeness,
    flags: Vec<QualityFlag>,
    sampling: Option<SamplingMetadata>,
    provider_uncertainty: Option<ProviderUncertainty>,
}

impl QualityMetadata {
    pub fn complete() -> Self {
        Self {
            completeness: Completeness::Complete,
            flags: Vec::new(),
            sampling: None,
            provider_uncertainty: None,
        }
    }

    pub fn partial() -> Self {
        Self {
            completeness: Completeness::Partial,
            flags: Vec::new(),
            sampling: None,
            provider_uncertainty: None,
        }
    }

    pub fn with_flag(mut self, flag: QualityFlag) -> Self {
        if !self.flags.contains(&flag) {
            self.flags.push(flag);
            self.flags.sort_unstable();
        }
        self
    }

    pub fn with_sampling(mut self, sampling: SamplingMetadata) -> Self {
        self.sampling = Some(sampling);
        self
    }

    pub fn with_provider_uncertainty(mut self, uncertainty: ProviderUncertainty) -> Self {
        self.provider_uncertainty = Some(uncertainty);
        if !self
            .flags
            .contains(&QualityFlag::ProviderReportedUncertainty)
        {
            self.flags.push(QualityFlag::ProviderReportedUncertainty);
            self.flags.sort_unstable();
        }
        self
    }

    pub fn validate(&self) -> Result<(), TelemetryError> {
        let uncertainty_flag = self
            .flags
            .contains(&QualityFlag::ProviderReportedUncertainty);
        if uncertainty_flag != self.provider_uncertainty.is_some() {
            return Err(TelemetryError::InvalidSamplingMetadata);
        }
        Ok(())
    }

    pub const fn completeness(&self) -> Completeness {
        self.completeness
    }

    pub fn flags(&self) -> &[QualityFlag] {
        &self.flags
    }

    pub const fn sampling(&self) -> Option<SamplingMetadata> {
        self.sampling
    }

    pub const fn provider_uncertainty(&self) -> Option<ProviderUncertainty> {
        self.provider_uncertainty
    }
}

impl Default for QualityMetadata {
    fn default() -> Self {
        Self::complete()
    }
}
