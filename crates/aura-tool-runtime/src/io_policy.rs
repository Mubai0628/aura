use aura_tool_contract::{
    MAX_ATTEMPT_LOGICAL_IO_BYTES, MAX_COMMAND_COMBINED_OUTPUT_BYTES, MAX_COMMAND_STDERR_BYTES,
    MAX_COMMAND_STDOUT_BYTES, MAX_HTTP_REQUEST_BODY_BYTES, MAX_HTTP_RESPONSE_DECOMPRESSED_BYTES,
    MAX_HTTP_RESPONSE_WIRE_BYTES, MAX_TOOL_ARTIFACT_BYTES,
};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrdinaryToolIoChannelV1 {
    CommandStdout,
    CommandStderr,
    HttpRequestBody,
    HttpResponseWire,
    HttpResponseDecompressed,
    Artifact,
    Result,
    Diagnostic,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum OrdinaryToolIoLimitErrorV1 {
    #[error("the channel-specific ordinary Tool I/O limit was exceeded")]
    ChannelExceeded { channel: OrdinaryToolIoChannelV1 },
    #[error("the combined command output limit was exceeded")]
    CommandCombinedExceeded,
    #[error("the logical per-attempt ordinary Tool I/O limit was exceeded")]
    AttemptAggregateExceeded,
    #[error("the logical I/O counter overflowed")]
    CounterOverflow,
}

/// Core-owned logical accounting for `OrdinaryToolIoPolicyV1`.
///
/// Every observation is charged even when the underlying content object is
/// deduplicated. Callers must abort the Runtime or HTTP stream on the first
/// error; truncating and returning success is forbidden.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OrdinaryToolIoMeterV1 {
    command_stdout: u64,
    command_stderr: u64,
    http_request_body: u64,
    http_response_wire: u64,
    http_response_decompressed: u64,
    artifact_bytes: u64,
    result_bytes: u64,
    diagnostic_bytes: u64,
    logical_total: u64,
}

impl OrdinaryToolIoMeterV1 {
    pub fn charge(
        &mut self,
        channel: OrdinaryToolIoChannelV1,
        bytes: u64,
    ) -> Result<(), OrdinaryToolIoLimitErrorV1> {
        let next_channel = match channel {
            OrdinaryToolIoChannelV1::CommandStdout => {
                checked_add(self.command_stdout, bytes, MAX_COMMAND_STDOUT_BYTES, channel)?
            }
            OrdinaryToolIoChannelV1::CommandStderr => {
                checked_add(self.command_stderr, bytes, MAX_COMMAND_STDERR_BYTES, channel)?
            }
            OrdinaryToolIoChannelV1::HttpRequestBody => {
                checked_add(self.http_request_body, bytes, MAX_HTTP_REQUEST_BODY_BYTES, channel)?
            }
            OrdinaryToolIoChannelV1::HttpResponseWire => {
                checked_add(self.http_response_wire, bytes, MAX_HTTP_RESPONSE_WIRE_BYTES, channel)?
            }
            OrdinaryToolIoChannelV1::HttpResponseDecompressed => checked_add(
                self.http_response_decompressed,
                bytes,
                MAX_HTTP_RESPONSE_DECOMPRESSED_BYTES,
                channel,
            )?,
            OrdinaryToolIoChannelV1::Artifact => {
                if bytes > u64::try_from(MAX_TOOL_ARTIFACT_BYTES).unwrap_or(u64::MAX) {
                    return Err(OrdinaryToolIoLimitErrorV1::ChannelExceeded { channel });
                }
                self.artifact_bytes
                    .checked_add(bytes)
                    .ok_or(OrdinaryToolIoLimitErrorV1::CounterOverflow)?
            }
            OrdinaryToolIoChannelV1::Result => self
                .result_bytes
                .checked_add(bytes)
                .ok_or(OrdinaryToolIoLimitErrorV1::CounterOverflow)?,
            OrdinaryToolIoChannelV1::Diagnostic => self
                .diagnostic_bytes
                .checked_add(bytes)
                .ok_or(OrdinaryToolIoLimitErrorV1::CounterOverflow)?,
        };

        let next_command_total = if matches!(
            channel,
            OrdinaryToolIoChannelV1::CommandStdout | OrdinaryToolIoChannelV1::CommandStderr
        ) {
            match channel {
                OrdinaryToolIoChannelV1::CommandStdout => {
                    next_channel.checked_add(self.command_stderr)
                }
                OrdinaryToolIoChannelV1::CommandStderr => {
                    self.command_stdout.checked_add(next_channel)
                }
                _ => unreachable!("matched command channel"),
            }
            .ok_or(OrdinaryToolIoLimitErrorV1::CounterOverflow)?
        } else {
            self.command_stdout
                .checked_add(self.command_stderr)
                .ok_or(OrdinaryToolIoLimitErrorV1::CounterOverflow)?
        };
        if next_command_total > u64::try_from(MAX_COMMAND_COMBINED_OUTPUT_BYTES).unwrap_or(u64::MAX)
        {
            return Err(OrdinaryToolIoLimitErrorV1::CommandCombinedExceeded);
        }

        let next_total = self
            .logical_total
            .checked_add(bytes)
            .ok_or(OrdinaryToolIoLimitErrorV1::CounterOverflow)?;
        if next_total > u64::try_from(MAX_ATTEMPT_LOGICAL_IO_BYTES).unwrap_or(u64::MAX) {
            return Err(OrdinaryToolIoLimitErrorV1::AttemptAggregateExceeded);
        }

        match channel {
            OrdinaryToolIoChannelV1::CommandStdout => self.command_stdout = next_channel,
            OrdinaryToolIoChannelV1::CommandStderr => self.command_stderr = next_channel,
            OrdinaryToolIoChannelV1::HttpRequestBody => self.http_request_body = next_channel,
            OrdinaryToolIoChannelV1::HttpResponseWire => self.http_response_wire = next_channel,
            OrdinaryToolIoChannelV1::HttpResponseDecompressed => {
                self.http_response_decompressed = next_channel;
            }
            OrdinaryToolIoChannelV1::Artifact => self.artifact_bytes = next_channel,
            OrdinaryToolIoChannelV1::Result => self.result_bytes = next_channel,
            OrdinaryToolIoChannelV1::Diagnostic => self.diagnostic_bytes = next_channel,
        }
        self.logical_total = next_total;
        Ok(())
    }

    #[must_use]
    pub const fn logical_total(&self) -> u64 {
        self.logical_total
    }
}

fn checked_add(
    current: u64,
    bytes: u64,
    maximum: usize,
    channel: OrdinaryToolIoChannelV1,
) -> Result<u64, OrdinaryToolIoLimitErrorV1> {
    let next = current.checked_add(bytes).ok_or(OrdinaryToolIoLimitErrorV1::CounterOverflow)?;
    if next > u64::try_from(maximum).unwrap_or(u64::MAX) {
        return Err(OrdinaryToolIoLimitErrorV1::ChannelExceeded { channel });
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_and_combined_limits_are_hard_failures_without_partial_charge() {
        let mut meter = OrdinaryToolIoMeterV1::default();
        meter
            .charge(
                OrdinaryToolIoChannelV1::CommandStdout,
                u64::try_from(MAX_COMMAND_STDOUT_BYTES).expect("limit"),
            )
            .expect("exact stdout limit");
        assert_eq!(
            meter.charge(OrdinaryToolIoChannelV1::CommandStdout, 1),
            Err(OrdinaryToolIoLimitErrorV1::ChannelExceeded {
                channel: OrdinaryToolIoChannelV1::CommandStdout,
            })
        );
        assert_eq!(meter.logical_total(), u64::try_from(MAX_COMMAND_STDOUT_BYTES).expect("limit"));
        meter
            .charge(
                OrdinaryToolIoChannelV1::CommandStderr,
                u64::try_from(MAX_COMMAND_STDERR_BYTES).expect("limit"),
            )
            .expect("combined exact limit");
        assert_eq!(
            meter.charge(OrdinaryToolIoChannelV1::CommandStderr, 1),
            Err(OrdinaryToolIoLimitErrorV1::ChannelExceeded {
                channel: OrdinaryToolIoChannelV1::CommandStderr,
            })
        );
    }

    #[test]
    fn artifacts_are_limited_per_item_and_charged_logically() {
        let mut meter = OrdinaryToolIoMeterV1::default();
        assert_eq!(
            meter.charge(
                OrdinaryToolIoChannelV1::Artifact,
                u64::try_from(MAX_TOOL_ARTIFACT_BYTES).expect("limit") + 1,
            ),
            Err(OrdinaryToolIoLimitErrorV1::ChannelExceeded {
                channel: OrdinaryToolIoChannelV1::Artifact,
            })
        );
        meter.charge(OrdinaryToolIoChannelV1::Artifact, 4_096).expect("artifact one");
        meter
            .charge(OrdinaryToolIoChannelV1::Artifact, 4_096)
            .expect("same deduplicated content is still logically charged");
        assert_eq!(meter.logical_total(), 8_192);
    }
}
