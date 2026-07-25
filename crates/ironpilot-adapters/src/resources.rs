use core::fmt;

use ironpilot_application::{ResourceSample, UnixMillis};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

#[derive(Debug)]
pub struct ProcessResourceSampler {
    process_id: Pid,
    system: System,
}

impl ProcessResourceSampler {
    pub fn new() -> Result<Self, ResourceSampleError> {
        let process_id =
            sysinfo::get_current_pid().map_err(|_| ResourceSampleError::ProcessIdUnavailable)?;
        Ok(Self {
            process_id,
            system: System::new(),
        })
    }

    pub fn sample(
        &mut self,
        observed_at: UnixMillis,
    ) -> Result<ResourceSample, ResourceSampleError> {
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[self.process_id]),
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        let process = self
            .system
            .process(self.process_id)
            .ok_or(ResourceSampleError::CurrentProcessUnavailable)?;
        let cpu_usage_percent = process.cpu_usage();
        if !cpu_usage_percent.is_finite() || cpu_usage_percent.is_sign_negative() {
            return Err(ResourceSampleError::InvalidCpuUsage);
        }
        Ok(ResourceSample::new(
            observed_at,
            process.memory(),
            cpu_usage_percent,
        ))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceSampleError {
    ProcessIdUnavailable,
    CurrentProcessUnavailable,
    InvalidCpuUsage,
}

impl fmt::Display for ResourceSampleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProcessIdUnavailable => formatter.write_str("current process ID is unavailable"),
            Self::CurrentProcessUnavailable => {
                formatter.write_str("current process resource metrics are unavailable")
            }
            Self::InvalidCpuUsage => {
                formatter.write_str("current process CPU metric is not finite")
            }
        }
    }
}

impl std::error::Error for ResourceSampleError {}
