use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime};
use thiserror::Error;

pub const TELEMETRY_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WorldPosition {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TelemetryPacket {
    pub protocol_version: u32,
    pub game: String,
    pub session_id: String,
    pub sequence: u64,
    pub timestamp_unix_ms: u128,
    pub world: WorldPosition,
    pub heading: Option<f64>,
    pub paused: bool,
}

#[derive(Debug, Error, PartialEq)]
pub enum TelemetryError {
    #[error("malformed telemetry JSON")]
    Malformed,
    #[error("unsupported telemetry protocol version")]
    UnsupportedVersion,
    #[error("telemetry packet is not from ETS2")]
    WrongGame,
    #[error("telemetry session id is missing or too long")]
    InvalidSession,
    #[error("telemetry contains a non-finite world value")]
    NonFiniteWorld,
    #[error("telemetry sequence is stale")]
    StaleSequence,
}

impl TelemetryPacket {
    pub fn validate(&self) -> Result<(), TelemetryError> {
        if self.protocol_version != TELEMETRY_PROTOCOL_VERSION {
            return Err(TelemetryError::UnsupportedVersion);
        }
        if self.game != "ets2" {
            return Err(TelemetryError::WrongGame);
        }
        if self.session_id.is_empty() || self.session_id.len() > 128 {
            return Err(TelemetryError::InvalidSession);
        }
        let world = [self.world.x, self.world.y, self.world.z];
        if !world.iter().all(|value| value.is_finite())
            || self.heading.is_some_and(|value| !value.is_finite())
        {
            return Err(TelemetryError::NonFiniteWorld);
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct TelemetryState {
    session_id: Option<String>,
    last_sequence: Option<u64>,
    last_packet: Option<TelemetryPacket>,
    last_received: Option<SystemTime>,
}

impl TelemetryState {
    pub fn ingest(&mut self, bytes: &[u8]) -> Result<&TelemetryPacket, TelemetryError> {
        let packet: TelemetryPacket =
            serde_json::from_slice(bytes).map_err(|_| TelemetryError::Malformed)?;
        packet.validate()?;
        if self.session_id.as_deref() == Some(packet.session_id.as_str())
            && self
                .last_sequence
                .is_some_and(|last| packet.sequence <= last)
        {
            return Err(TelemetryError::StaleSequence);
        }
        self.session_id = Some(packet.session_id.clone());
        self.last_sequence = Some(packet.sequence);
        self.last_packet = Some(packet);
        self.last_received = Some(SystemTime::now());
        Ok(self.last_packet.as_ref().expect("packet stored above"))
    }

    pub fn current(&self) -> Option<&TelemetryPacket> {
        self.last_packet.as_ref()
    }

    pub fn is_stale(&self, max_age: Duration) -> bool {
        self.last_received
            .and_then(|received| SystemTime::now().duration_since(received).ok())
            .is_none_or(|age| age > max_age)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(sequence: u64) -> TelemetryPacket {
        TelemetryPacket {
            protocol_version: 1,
            game: "ets2".into(),
            session_id: "test-session".into(),
            sequence,
            timestamp_unix_ms: 1,
            world: WorldPosition {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
            heading: None,
            paused: false,
        }
    }

    #[test]
    fn accepts_valid_packet_and_rejects_stale_sequence() {
        let mut state = TelemetryState::default();
        let first = serde_json::to_vec(&packet(4)).unwrap();
        assert!(state.ingest(&first).is_ok());
        assert_eq!(state.ingest(&first), Err(TelemetryError::StaleSequence));
    }

    #[test]
    fn a_new_session_may_restart_sequence() {
        let mut state = TelemetryState::default();
        state
            .ingest(&serde_json::to_vec(&packet(99)).unwrap())
            .unwrap();
        let mut next = packet(0);
        next.session_id = "new-session".into();
        assert!(state.ingest(&serde_json::to_vec(&next).unwrap()).is_ok());
    }

    #[test]
    fn rejects_wrong_game_and_non_finite_values() {
        let mut wrong = packet(0);
        wrong.game = "ats".into();
        assert_eq!(wrong.validate(), Err(TelemetryError::WrongGame));
        let mut invalid = packet(0);
        invalid.world.x = f64::NAN;
        assert_eq!(invalid.validate(), Err(TelemetryError::NonFiniteWorld));
    }
}
