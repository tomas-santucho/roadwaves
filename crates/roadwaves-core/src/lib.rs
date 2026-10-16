//! Cross-platform RoadWaves core: provider parsing, station selection, and safe ETS2 integration.

pub mod audio;
pub mod autotune;
pub mod cache;
pub mod config;
pub mod discovery;
pub mod game;
pub mod geo;
pub mod platform;
pub mod selection;
pub mod telemetry;

pub use autotune::{AutoTune, TuneAction, TuneMode};
pub use discovery::{is_safe_stream_url, RadioGardenClient, Station, StationProviderError};
pub use game::{GameInstall, IntegrationReport, SiiEditor};
pub use geo::{
    Calibration, CalibrationError, GameGeoResolver, LocationConfidence, LocationResolution,
    LocationSource, WorldAnchor,
};
pub use platform::{discover_ets2_game_bins, discover_ets2_profiles};
pub use selection::{
    choose_station, choose_station_for_country, GeoPoint, SelectionConfig, SelectionState,
};
pub use telemetry::{TelemetryError, TelemetryPacket, TelemetryState, WorldPosition};
