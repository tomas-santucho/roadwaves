use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use roadwaves_core::{
    config, discover_ets2_game_bins, discover_ets2_profiles, is_safe_stream_url, AutoTune,
    Calibration, GameGeoResolver, RadioGardenClient, SiiEditor, TelemetryState, TuneAction,
    TuneMode, WorldAnchor,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream, UdpSocket},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Parser)]
#[command(
    name = "roadwaves",
    version,
    about = "RoadWaves ETS2 local radio integration"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Doctor {
        #[arg(long)]
        json: bool,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Providers {
        #[command(subcommand)]
        command: ProviderCommand,
    },
    Radio {
        #[command(subcommand)]
        command: RadioCommand,
    },
    Discover,
    Channels {
        #[arg(long)]
        place_id: String,
    },
    ProbeStream {
        #[arg(long)]
        channel_id: String,
    },
    FindGame,
    Serve {
        #[arg(long, default_value = "127.0.0.1:47926")]
        bind: String,
        #[arg(long)]
        upstream: Option<String>,
        #[arg(long)]
        calibration: Option<PathBuf>,
        #[arg(long, hide = true)]
        fixture: bool,
        #[arg(long, hide = true)]
        control_token: Option<String>,
    },
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
    },
    Simulate {
        #[arg(long)]
        route: PathBuf,
        #[arg(long, default_value_t = 10.0)]
        speed: f64,
    },
    Calibrate {
        #[arg(long)]
        anchors: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Install {
        #[arg(long)]
        radio_file: PathBuf,
        #[arg(long, default_value = "http://127.0.0.1:47926/radio.mp3")]
        endpoint: String,
    },
    InstallPlugin {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        game_bin: PathBuf,
    },
    InstallIntegration {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        game_bin: PathBuf,
        #[arg(long)]
        radio_file: PathBuf,
        #[arg(long, default_value = "http://127.0.0.1:47926/radio.mp3")]
        endpoint: String,
    },
    Repair {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        game_bin: PathBuf,
        #[arg(long)]
        radio_file: PathBuf,
        #[arg(long, default_value = "http://127.0.0.1:47926/radio.mp3")]
        endpoint: String,
    },
    UninstallPlugin {
        #[arg(long)]
        game_bin: PathBuf,
    },
    UninstallIntegration {
        #[arg(long)]
        game_bin: PathBuf,
        #[arg(long)]
        radio_file: PathBuf,
    },
    Uninstall {
        #[arg(long)]
        radio_file: PathBuf,
    },
}

#[derive(Subcommand)]
enum ConfigCommand {
    Show,
}

#[derive(Subcommand)]
enum ProviderCommand {
    Status {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum RadioCommand {
    Test {
        #[arg(long)]
        channel_id: String,
    },
}

#[derive(Subcommand)]
enum ServiceCommand {
    Status {
        #[arg(long)]
        json: bool,
    },
    Run {
        #[arg(long, default_value = "127.0.0.1:47926")]
        bind: String,
        #[arg(long)]
        upstream: Option<String>,
        #[arg(long)]
        calibration: Option<PathBuf>,
        #[arg(long, hide = true)]
        fixture: bool,
        #[arg(long, hide = true)]
        control_token: Option<String>,
    },
}
fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Config {
            command: ConfigCommand::Show,
        } => {
            let path = roadwaves_core::config::user_config_path();
            let loaded = roadwaves_core::config::load(&path)?;
            println!(
                "{}",
                serde_json::json!({
                    "path": path,
                    "warning": loaded.warning,
                    "config": loaded.config,
                })
            );
        }
        Command::Doctor { json } => {
            if json {
                println!("{}", serde_json::to_string_pretty(&doctor_report())?);
                return Ok(());
            }
            println!("RoadWaves doctor");
            println!("provider: Radio Garden only");
            println!("audio endpoint: http://127.0.0.1:47926/radio.mp3");
            println!("telemetry receiver: udp://127.0.0.1:47927");
            let profiles = discover_ets2_profiles();
            println!("ets2 profiles found: {}", profiles.len());
            for profile in profiles {
                println!("  [{}] {}", profile.mode, profile.radio_file.display());
            }
            println!(
                "ets2 game binaries found: {}",
                discover_ets2_game_bins().len()
            );
            let doctor = doctor_report();
            println!(
                "local service: {}",
                if doctor.service_reachable {
                    "reachable"
                } else {
                    "unreachable"
                }
            );
            println!(
                "RoadWaves radio entries: {} | plugins: {}",
                doctor.radio_entries.len(),
                doctor.plugin_targets.len()
            );
            println!("status: local core available; live game files unchanged");
        }
        Command::Providers {
            command: ProviderCommand::Status { json },
        } => provider_status(json)?,
        Command::Radio {
            command: RadioCommand::Test { channel_id },
        } => {
            let stream = RadioGardenClient::default()
                .probe_channel_stream(&channel_id)
                .context("Radio Garden stream was unavailable, unsafe, or not MP3-compatible")?;
            println!(
                "channel_id={} stream_url={} content_type={}",
                channel_id,
                stream.final_url,
                stream.content_type.unwrap_or_default()
            );
        }
        Command::Discover => {
            let cached = RadioGardenClient::default()
                .places_with_cache(&roadwaves_core::cache::user_cache_dir().join("places.json"))
                .context("Radio Garden is unavailable or denied; no alternate provider is used")?;
            println!(
                "{} places discovered{}",
                cached.places.len(),
                if cached.from_cache {
                    if cached.stale {
                        " (cached metadata, stale)"
                    } else {
                        " (cached metadata)"
                    }
                } else {
                    ""
                }
            );
            for s in cached.places.into_iter().take(10) {
                println!(
                    "{}: {} ({:.4},{:.4})",
                    s.id, s.name, s.location.latitude, s.location.longitude
                );
            }
        }
        Command::Channels { place_id } => {
            let channels = RadioGardenClient::default().channels_for_place(&place_id)
                .context("Radio Garden channel discovery is unavailable or changed; no alternate provider is used")?;
            println!("{} channels", channels.len());
            for channel in channels {
                println!("{}: {}", channel.id, channel.name);
            }
        }
        Command::ProbeStream { channel_id } => {
            let stream = RadioGardenClient::default()
                .probe_channel_stream(&channel_id)
                .context("Radio Garden stream was unavailable, unsafe, or not MP3-compatible")?;
            println!(
                "stream_url={} content_type={}",
                stream.final_url,
                stream.content_type.unwrap_or_default()
            );
        }
        Command::FindGame => {
            println!(
                "{}",
                serde_json::to_string_pretty(&discover_ets2_profiles())?
            );
        }
        Command::Serve {
            bind,
            upstream,
            calibration,
            fixture,
            control_token,
        } => serve(
            &bind,
            upstream.as_deref(),
            calibration.as_deref(),
            fixture,
            control_token.as_deref(),
        )?,
        Command::Service {
            command: ServiceCommand::Status { json },
        } => service_status(json)?,
        Command::Service {
            command:
                ServiceCommand::Run {
                    bind,
                    upstream,
                    calibration,
                    fixture,
                    control_token,
                },
        } => serve(
            &bind,
            upstream.as_deref(),
            calibration.as_deref(),
            fixture,
            control_token.as_deref(),
        )?,
        Command::Simulate { route, speed } => simulate(&route, speed)?,
        Command::Calibrate { anchors, output } => calibrate(&anchors, output.as_deref())?,
        Command::Install {
            radio_file,
            endpoint,
        } => println!(
            "{}",
            serde_json::to_string_pretty(&SiiEditor::install(&radio_file, &endpoint)?)?
        ),
        Command::InstallPlugin { source, game_bin } => println!(
            "{}",
            serde_json::to_string_pretty(&SiiEditor::install_plugin(&source, &game_bin)?)?
        ),
        Command::InstallIntegration {
            source,
            game_bin,
            radio_file,
            endpoint,
        } => {
            let (plugin, radio) =
                SiiEditor::install_integration(&source, &game_bin, &radio_file, &endpoint)?;
            println!(
                "{}",
                serde_json::json!({ "plugin": plugin, "radio": radio })
            );
        }
        Command::Repair {
            source,
            game_bin,
            radio_file,
            endpoint,
        } => {
            let (plugin, radio) =
                SiiEditor::install_integration(&source, &game_bin, &radio_file, &endpoint)?;
            println!(
                "{}",
                serde_json::json!({ "plugin": plugin, "radio": radio })
            );
        }
        Command::UninstallPlugin { game_bin } => println!(
            "{}",
            serde_json::to_string_pretty(&SiiEditor::uninstall_plugin(&game_bin)?)?
        ),
        Command::UninstallIntegration {
            game_bin,
            radio_file,
        } => {
            let (radio, plugin) = SiiEditor::uninstall_integration(&radio_file, &game_bin)?;
            println!(
                "{}",
                serde_json::json!({ "radio": radio, "plugin": plugin })
            );
        }
        Command::Uninstall { radio_file } => println!(
            "{}",
            serde_json::to_string_pretty(&SiiEditor::uninstall(&radio_file)?)?
        ),
    }
    Ok(())
}

fn provider_status(json: bool) -> Result<()> {
    let path = roadwaves_core::cache::user_cache_dir().join("places.json");
    let result = RadioGardenClient::default().places_with_cache(&path);
    match result {
        Ok(cached) => {
            let status = if cached.from_cache {
                if cached.stale {
                    "cached-stale"
                } else {
                    "cached"
                }
            } else {
                "live"
            };
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "provider": "Radio Garden",
                        "status": status,
                        "places": cached.places.len(),
                        "stale": cached.stale,
                        "live_streams_require_network": true,
                    })
                );
            } else {
                println!(
                    "provider=Radio Garden status={status} places={} live_streams_require_network=true",
                    cached.places.len()
                );
            }
        }
        Err(error) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "provider": "Radio Garden",
                        "status": "unavailable",
                        "error": error.to_string(),
                        "live_streams_require_network": true,
                    })
                );
            } else {
                println!(
                    "provider=Radio Garden status=unavailable error={} live_streams_require_network=true",
                    error
                );
            }
        }
    }
    Ok(())
}

fn service_status(json: bool) -> Result<()> {
    let mut stream = TcpStream::connect_timeout(
        &"127.0.0.1:47926"
            .parse::<SocketAddr>()
            .expect("numeric loopback address is valid"),
        Duration::from_secs(2),
    )
    .context("RoadWaves service is not reachable on loopback port 47926")?;
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .context("could not configure service status timeout")?;
    stream
        .write_all(b"GET /status HTTP/1.1\r\nHost: 127.0.0.1:47926\r\nConnection: close\r\n\r\n")?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .context("service returned an invalid HTTP response")?;
    if !headers.starts_with("HTTP/1.1 200") {
        bail!(
            "service returned {}",
            headers.lines().next().unwrap_or("error")
        );
    }
    let status: serde_json::Value =
        serde_json::from_str(body).context("service returned invalid status JSON")?;
    if json {
        println!("{}", serde_json::to_string_pretty(&status)?);
    } else {
        let text = |key: &str| {
            status
                .get(key)
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown")
        };
        println!(
            "mode={} station={} country={} city={} provider={} confidence={} provider_error={}",
            text("mode"),
            text("current_station"),
            text("current_country"),
            text("current_city"),
            text("provider"),
            text("confidence"),
            text("provider_error")
        );
    }
    Ok(())
}

fn serve(
    bind: &str,
    upstream: Option<&str>,
    calibration_path: Option<&std::path::Path>,
    fixture: bool,
    control_token: Option<&str>,
) -> Result<()> {
    let bind_address = loopback_bind_address(bind)?;
    if let Some(url) = upstream {
        if !is_safe_stream_url(url) {
            bail!("upstream URL is not a safe HTTP(S) stream URL");
        }
        println!("RoadWaves upstream relay: {url}");
    } else if fixture {
        println!("RoadWaves upstream relay: local MP3 fixture");
    } else {
        println!("RoadWaves upstream relay: waiting for a live Radio Garden selection");
    }
    let telemetry = UdpSocket::bind("127.0.0.1:47927")
        .context("could not bind localhost telemetry receiver")?;
    telemetry.set_read_timeout(Some(Duration::from_millis(500)))?;
    println!("RoadWaves telemetry receiver: udp://127.0.0.1:47927");
    let default_calibration_path = roadwaves_core::cache::user_cache_dir().join("calibration.json");
    let calibration_path = calibration_path.unwrap_or(&default_calibration_path);
    let resolver = std::fs::read(calibration_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Calibration>(&bytes).ok())
        .map(GameGeoResolver::from_calibration)
        .unwrap_or_else(GameGeoResolver::ets2_default);
    let config_path = config::user_config_path();
    let app_config = config::load(&config_path)
        .map(|loaded| loaded.config)
        .unwrap_or_default();
    let favorite_ids: HashSet<String> = app_config
        .favorites
        .iter()
        .map(|favorite| favorite.channel_id.clone())
        .collect();
    let hidden_ids: HashSet<String> = app_config.hidden_channels.iter().cloned().collect();
    let selected_stream = Arc::new(Mutex::new(None::<String>));
    let service_status = Arc::new(Mutex::new(ServiceStatus {
        mode: format!("{:?}", app_config.mode),
        provider: "Radio Garden".into(),
        ..ServiceStatus::default()
    }));
    let service_status_for_telemetry = Arc::clone(&service_status);
    let selected_stream_for_telemetry = Arc::clone(&selected_stream);
    let cache_path = roadwaves_core::cache::user_cache_dir().join("places.json");
    let allow_dynamic_discovery = upstream.is_none();
    let (control_sender, control_receiver) = mpsc::channel();
    let stopping = Arc::new(AtomicBool::new(false));
    let stopping_for_handler = Arc::clone(&stopping);
    ctrlc::set_handler(move || {
        stopping_for_handler.store(true, Ordering::Release);
    })
    .context("could not install service shutdown handler")?;
    let stopping_for_telemetry = Arc::clone(&stopping);
    thread::spawn(move || {
        let mut packet = [0u8; 512];
        let mut state = TelemetryState::default();
        let mut tuner = AutoTune::default();
        tuner.set_mode(app_config.mode);
        let mut favorite_ids = favorite_ids;
        let mut hidden_ids = hidden_ids;
        let catalog = RadioGardenClient::default();
        let mut last_discovery = None::<Instant>;
        let mut last_discovery_geo = None::<roadwaves_core::GeoPoint>;
        let mut last_geo = None::<roadwaves_core::GeoPoint>;
        let mut last_session = None::<String>;
        let mut playable = Vec::new();
        let mut manual_location = None::<roadwaves_core::GeoPoint>;
        while !stopping_for_telemetry.load(Ordering::Acquire) {
            while let Ok(command) = control_receiver.try_recv() {
                handle_control_command(
                    command,
                    &mut tuner,
                    &mut playable,
                    &selected_stream_for_telemetry,
                    &service_status_for_telemetry,
                    &mut manual_location,
                    &mut last_discovery,
                    &mut favorite_ids,
                    &mut hidden_ids,
                    &config_path,
                );
            }
            match telemetry.recv_from(&mut packet) {
                Ok((size, source)) => match state.ingest(&packet[..size]) {
                    Ok(parsed) => {
                        let mut location = resolver.resolve(parsed.world.x, parsed.world.z);
                        if let Some(override_location) = manual_location {
                            location.estimated_geo = Some(override_location);
                        }
                        let session_changed = last_session
                            .as_deref()
                            .is_some_and(|session| session != parsed.session_id)
                            || last_session.is_none();
                        let moved_km = location
                            .estimated_geo
                            .zip(last_geo)
                            .map(|(current, previous)| current.distance_km(previous));
                        let meaningful_move = moved_km.is_some_and(|distance| distance >= 20.0);
                        let quick_travel = moved_km.is_some_and(|distance| distance >= 100.0);
                        if session_changed || quick_travel || meaningful_move {
                            tuner.reset_location_cooldown();
                        }
                        last_session = Some(parsed.session_id.clone());
                        if let Some(geo) = location.estimated_geo {
                            last_geo = Some(geo);
                        }
                        if let Ok(mut status) = service_status_for_telemetry.lock() {
                            status.mode = format!("{:?}", tuner.mode);
                            status.last_sequence = Some(parsed.sequence);
                            status.session_id = Some(parsed.session_id.clone());
                            status.location = location.estimated_geo;
                            status.confidence = if manual_location.is_some() {
                                "ManualOverride".into()
                            } else {
                                format!("{:?}", location.confidence)
                            };
                        }
                        if let Some(geo) = location.estimated_geo {
                            if allow_dynamic_discovery
                                && (last_discovery
                                    .is_none_or(|last| last.elapsed() >= Duration::from_secs(60))
                                    || last_discovery_geo
                                        .is_some_and(|previous| geo.distance_km(previous) >= 20.0))
                            {
                                last_discovery = Some(Instant::now());
                                match catalog.discover_nearby_playable(geo, &cache_path) {
                                    Ok(mut discovered_playable) => {
                                        last_discovery_geo = Some(geo);
                                        for entry in &mut discovered_playable {
                                            entry.candidate.favorite =
                                                favorite_ids.contains(&entry.candidate.id);
                                            entry.candidate.hidden =
                                                hidden_ids.contains(&entry.candidate.id);
                                        }
                                        let candidates: Vec<_> = discovered_playable
                                            .iter()
                                            .map(|entry| entry.candidate.clone())
                                            .collect();
                                        if tuner.mode == TuneMode::FavoriteLock
                                            && tuner.state.current_id.is_none()
                                        {
                                            tuner.state.current_id = candidates
                                                .iter()
                                                .find(|candidate| candidate.favorite)
                                                .map(|candidate| candidate.id.clone());
                                        }
                                        playable = discovered_playable;
                                        if let TuneAction::Switched(chosen) =
                                            tuner.update(Instant::now(), geo, &candidates)
                                        {
                                            if let Some(entry) = playable
                                                .iter()
                                                .find(|entry| entry.candidate.id == chosen.id)
                                            {
                                                if let Ok(mut current) =
                                                    selected_stream_for_telemetry.lock()
                                                {
                                                    *current = Some(entry.stream.final_url.clone());
                                                }
                                                if let Ok(mut status) =
                                                    service_status_for_telemetry.lock()
                                                {
                                                    status.current_channel_id =
                                                        Some(chosen.id.clone());
                                                    status.current_station =
                                                        Some(chosen.name.clone());
                                                    status.current_country = chosen.country.clone();
                                                    status.current_city = chosen.city.clone();
                                                    status.provider_error = None;
                                                    status.provider = "Radio Garden".into();
                                                }
                                                println!(
                                                    "radio: selected {} ({}) score-driven local candidate",
                                                    chosen.name, chosen.id
                                                );
                                            }
                                        }
                                    }
                                    Err(error) => {
                                        if let Ok(mut status) = service_status_for_telemetry.lock()
                                        {
                                            status.provider_error = Some(error.to_string());
                                        }
                                        println!("radio: provider unavailable: {error}");
                                    }
                                }
                            }
                            println!(
                                "telemetry: accepted seq={} session={} world=({:.2},{:.2},{:.2}) geo=({:.5},{:.5}) confidence={:?} source={}",
                                parsed.sequence, parsed.session_id, parsed.world.x, parsed.world.y,
                                parsed.world.z, geo.latitude, geo.longitude, location.confidence,
                                source.ip()
                            );
                        } else {
                            println!(
                                "telemetry: accepted seq={} session={} world=({:.2},{:.2},{:.2}) geo=unknown confidence={:?} source={}",
                                parsed.sequence, parsed.session_id, parsed.world.x, parsed.world.y,
                                parsed.world.z, location.confidence, source.ip()
                            );
                        }
                    }
                    Err(error) => println!("telemetry: rejected packet: {error}"),
                },
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        || error.kind() == std::io::ErrorKind::TimedOut => {}
                Err(_) => break,
            }
        }
    });
    let listener =
        TcpListener::bind(bind_address).context("could not bind localhost audio endpoint")?;
    listener.set_nonblocking(true)?;
    println!("RoadWaves audio endpoint: http://{bind_address}/radio.mp3");
    let active_clients = Arc::new(AtomicUsize::new(0));
    while !stopping.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                if active_clients.load(Ordering::Acquire) >= 4 {
                    let _ = stream.write_all(
                        b"HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\n\r\n",
                    );
                    continue;
                }
                active_clients.fetch_add(1, Ordering::AcqRel);
                let active_clients_for_worker = Arc::clone(&active_clients);
                let selected_stream_for_worker = Arc::clone(&selected_stream);
                let upstream = upstream.map(str::to_owned);
                let control_sender_for_worker = control_sender.clone();
                let control_token = control_token.map(str::to_owned);
                let service_status_for_worker = Arc::clone(&service_status);
                thread::spawn(move || {
                    let _ = handle_client(
                        &mut stream,
                        upstream.as_deref(),
                        &selected_stream_for_worker,
                        &service_status_for_worker,
                        Some(&control_sender_for_worker),
                        control_token.as_deref(),
                        fixture,
                    );
                    active_clients_for_worker.fetch_sub(1, Ordering::AcqRel);
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(error.into()),
        }
    }
    println!("RoadWaves service stopped.");
    Ok(())
}

fn loopback_bind_address(value: &str) -> Result<SocketAddr> {
    let address: SocketAddr = value
        .parse()
        .with_context(|| format!("bind address must be a numeric loopback address: {value}"))?;
    if !address.ip().is_loopback() {
        bail!("audio endpoint must bind to a loopback address, got {value}");
    }
    Ok(address)
}

fn handle_client(
    stream: &mut TcpStream,
    upstream: Option<&str>,
    selected_stream: &Mutex<Option<String>>,
    service_status: &Mutex<ServiceStatus>,
    control_sender: Option<&Sender<ControlCommand>>,
    control_token: Option<&str>,
    fixture: bool,
) -> Result<()> {
    let mut request = [0u8; 8192];
    let n = read_http_request(stream, &mut request)?;
    let first = std::str::from_utf8(&request[..n])
        .unwrap_or("")
        .lines()
        .next()
        .unwrap_or("");
    if first.starts_with("POST /control ") {
        handle_control_http(stream, &request[..n], control_sender, control_token)?;
    } else if first.starts_with("GET /health ") {
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{\"ok\":true,\"provider\":\"radio-garden\"}")?;
    } else if first.starts_with("GET /status ") {
        let status = service_status
            .lock()
            .map_err(|_| anyhow::anyhow!("service status unavailable"))?
            .clone();
        let body = serde_json::to_string(&status)?;
        write_json_response(stream, &body)?;
    } else if first.starts_with("GET /radio.mp3 ") {
        let selected = selected_stream
            .lock()
            .ok()
            .and_then(|stream| stream.clone());
        if let Some(upstream) = upstream.or(selected.as_deref()) {
            let client = reqwest::blocking::Client::builder()
                // A radio stream is intentionally long-lived. Bound connection setup,
                // but do not apply a total response timeout to the audio body.
                .timeout(None)
                .connect_timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::custom(|attempt| {
                    if attempt.previous().len() >= 4 {
                        attempt.stop()
                    } else if is_safe_stream_url(attempt.url().as_str()) {
                        attempt.follow()
                    } else {
                        attempt.error("unsafe redirect destination")
                    }
                }))
                .build()?;
            let mut response = match client.get(upstream).send() {
                Ok(response) => response,
                Err(error) => {
                    record_provider_error(service_status, &error.to_string());
                    request_discovery_refresh(control_sender);
                    return Err(error.into());
                }
            };
            if !response.status().is_success() {
                record_provider_error(
                    service_status,
                    &format!("upstream returned {}", response.status()),
                );
                request_discovery_refresh(control_sender);
                bail!("upstream returned {}", response.status());
            }
            let mut prefix = [0u8; 4];
            let count = match response.read(&mut prefix) {
                Ok(count) => count,
                Err(error) => {
                    record_provider_error(service_status, &error.to_string());
                    request_discovery_refresh(control_sender);
                    return Err(error.into());
                }
            };
            if !roadwaves_core::audio::is_mp3_prefix(&prefix[..count]) {
                record_provider_error(service_status, "upstream is not an MP3 stream");
                request_discovery_refresh(control_sender);
                bail!("upstream is not an MP3 stream");
            }
            if let Ok(mut status) = service_status.lock() {
                status.provider_error = None;
            }
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n")?;
            stream.write_all(&prefix[..count])?;
            let mut buffer = [0u8; 16 * 1024];
            loop {
                let read = match response.read(&mut buffer) {
                    Ok(read) => read,
                    Err(error) => {
                        record_provider_error(service_status, &error.to_string());
                        request_discovery_refresh(control_sender);
                        return Err(error.into());
                    }
                };
                if read == 0 {
                    break;
                }
                stream.write_all(&buffer[..read])?;
            }
        } else if fixture {
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n")?;
            stream.write_all(roadwaves_core::audio::SILENT_MP3)?;
        } else {
            record_provider_error(service_status, "no live Radio Garden stream selected");
            stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{\"ok\":false,\"error\":\"no live Radio Garden stream selected\"}")?;
        }
    } else {
        stream.write_all(b"HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n")?;
    }
    Ok(())
}

fn record_provider_error(service_status: &Mutex<ServiceStatus>, error: &str) {
    if let Ok(mut status) = service_status.lock() {
        status.provider_error = Some(error.to_owned());
    }
}

fn request_discovery_refresh(control_sender: Option<&Sender<ControlCommand>>) {
    let Some(control_sender) = control_sender else {
        return;
    };
    let (response_sender, _response_receiver) = mpsc::channel();
    let _ = control_sender.send(ControlCommand::Refresh(response_sender));
}

fn header_value<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let (key, value) = line.split_once(':')?;
    key.trim()
        .eq_ignore_ascii_case(name)
        .then_some(value.trim())
}

fn read_http_request(stream: &mut TcpStream, buffer: &mut [u8]) -> Result<usize> {
    let mut length = stream.read(buffer)?;
    let header_end = loop {
        if let Some(end) = buffer[..length]
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
        {
            break end + 4;
        }
        if length == buffer.len() {
            bail!("HTTP request headers exceeded the safety limit");
        }
        let read = stream.read(&mut buffer[length..])?;
        if read == 0 {
            return Ok(length);
        }
        length += read;
    };
    let header = std::str::from_utf8(&buffer[..header_end]).unwrap_or("");
    let content_length = header
        .lines()
        .find_map(|line| header_value(line, "Content-Length")?.parse::<usize>().ok())
        .unwrap_or(0);
    if content_length > 4096 {
        bail!("HTTP request body exceeded the safety limit");
    }
    let required = header_end.saturating_add(content_length);
    if required > buffer.len() {
        bail!("HTTP request exceeded the safety limit");
    }
    while length < required {
        let read = stream.read(&mut buffer[length..required])?;
        if read == 0 {
            break;
        }
        length += read;
    }
    Ok(length)
}

enum ControlCommand {
    SetMode(TuneMode, Sender<String>),
    Next(Sender<String>),
    Previous(Sender<String>),
    Favorite(Sender<String>),
    HideCurrent(Sender<String>),
    SetHidden {
        channel_id: String,
        hidden: bool,
        response: Sender<String>,
    },
    SetLocation(Option<roadwaves_core::GeoPoint>, Sender<String>),
    Refresh(Sender<String>),
}

#[derive(Debug, Clone, Default, Serialize)]
struct ServiceStatus {
    mode: String,
    current_channel_id: Option<String>,
    current_station: Option<String>,
    current_country: Option<String>,
    current_city: Option<String>,
    provider: String,
    provider_error: Option<String>,
    session_id: Option<String>,
    last_sequence: Option<u64>,
    location: Option<roadwaves_core::GeoPoint>,
    confidence: String,
}

#[derive(Debug, Deserialize)]
struct ControlRequest {
    action: String,
    mode: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    channel_id: Option<String>,
}

fn handle_control_http(
    stream: &mut TcpStream,
    request: &[u8],
    sender: Option<&Sender<ControlCommand>>,
    expected_token: Option<&str>,
) -> Result<()> {
    let Some(expected_token) = expected_token else {
        stream.write_all(b"HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n")?;
        return Ok(());
    };
    let text = std::str::from_utf8(request).unwrap_or("");
    let token = text
        .lines()
        .find_map(|line| header_value(line, "X-RoadWaves-Token"));
    if token != Some(expected_token) {
        stream.write_all(b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n")?;
        return Ok(());
    }
    let Some(sender) = sender else {
        stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\n\r\n")?;
        return Ok(());
    };
    let Some(body) = text.split("\r\n\r\n").nth(1) else {
        stream.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n")?;
        return Ok(());
    };
    let request: ControlRequest = match serde_json::from_str(body) {
        Ok(request) => request,
        Err(_) => {
            stream.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n")?;
            return Ok(());
        }
    };
    let (response_sender, response_receiver): (Sender<String>, Receiver<String>) = mpsc::channel();
    let command = match request.action.as_str() {
        "mode" => match request.mode.as_deref() {
            Some("AutoTune") => ControlCommand::SetMode(TuneMode::AutoTune, response_sender),
            Some("Explore") => ControlCommand::SetMode(TuneMode::Explore, response_sender),
            Some("FavoriteLock") => {
                ControlCommand::SetMode(TuneMode::FavoriteLock, response_sender)
            }
            _ => {
                stream.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n")?;
                return Ok(());
            }
        },
        "next" => ControlCommand::Next(response_sender),
        "previous" => ControlCommand::Previous(response_sender),
        "favorite" => ControlCommand::Favorite(response_sender),
        "hide" => ControlCommand::HideCurrent(response_sender),
        "unhide" => {
            let Some(channel_id) = request.channel_id.filter(|id| !id.trim().is_empty()) else {
                stream.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n")?;
                return Ok(());
            };
            ControlCommand::SetHidden {
                channel_id,
                hidden: false,
                response: response_sender,
            }
        }
        "location" => {
            let (Some(latitude), Some(longitude)) = (request.latitude, request.longitude) else {
                stream.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n")?;
                return Ok(());
            };
            let location = roadwaves_core::GeoPoint {
                latitude,
                longitude,
            };
            if !location.valid() {
                stream.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n")?;
                return Ok(());
            }
            ControlCommand::SetLocation(Some(location), response_sender)
        }
        "clear_location" => ControlCommand::SetLocation(None, response_sender),
        "refresh" => ControlCommand::Refresh(response_sender),
        _ => {
            stream.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n")?;
            return Ok(());
        }
    };
    sender
        .send(command)
        .map_err(|_| anyhow::anyhow!("service control unavailable"))?;
    let result: String = response_receiver
        .recv_timeout(Duration::from_secs(2))
        .unwrap_or_else(|_| "{\"ok\":false,\"error\":\"control timeout\"}".into());
    write_json_response(stream, &result)
}

fn write_json_response(stream: &mut TcpStream, body: &str) -> Result<()> {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn handle_control_command(
    command: ControlCommand,
    tuner: &mut AutoTune,
    playable: &mut [roadwaves_core::discovery::PlayableCandidate],
    selected_stream: &Mutex<Option<String>>,
    service_status: &Mutex<ServiceStatus>,
    manual_location: &mut Option<roadwaves_core::GeoPoint>,
    last_discovery: &mut Option<Instant>,
    favorite_ids: &mut HashSet<String>,
    hidden_ids: &mut HashSet<String>,
    config_path: &Path,
) {
    let (action, response) = match command {
        ControlCommand::SetMode(mode, response) => {
            tuner.set_mode(mode);
            if let Ok(mut status) = service_status.lock() {
                status.mode = format!("{:?}", tuner.mode);
            }
            (format!("mode={mode:?}"), response)
        }
        ControlCommand::Next(response) => {
            let action = tuner.next(
                &playable
                    .iter()
                    .map(|entry| entry.candidate.clone())
                    .collect::<Vec<_>>(),
                Instant::now(),
            );
            apply_control_selection(action, playable, selected_stream, service_status, response)
        }
        ControlCommand::Previous(response) => {
            let action = tuner.previous(Instant::now());
            apply_control_selection(action, playable, selected_stream, service_status, response)
        }
        ControlCommand::Refresh(response) => {
            *last_discovery = None;
            ("refresh requested".into(), response)
        }
        ControlCommand::Favorite(response) => {
            let Some(current) = tuner.current.as_ref().cloned() else {
                let _ = response.send("{\"ok\":false,\"error\":\"no current station\"}".into());
                return;
            };
            let Some(entry) = playable
                .iter_mut()
                .find(|entry| entry.candidate.id == current.id)
            else {
                let _ = response.send(
                    "{\"ok\":false,\"error\":\"current station is not in the live candidate list\"}"
                        .into(),
                );
                return;
            };
            entry.candidate.favorite = !entry.candidate.favorite;
            if let Some(current) = tuner.current.as_mut() {
                current.favorite = entry.candidate.favorite;
            }
            if entry.candidate.favorite {
                favorite_ids.insert(entry.candidate.id.clone());
            } else {
                favorite_ids.remove(&entry.candidate.id);
            }
            if let Err(error) = persist_favorite(
                config_path,
                &entry.candidate.id,
                &entry.candidate.name,
                entry.candidate.country.clone(),
                entry.candidate.favorite,
            ) {
                let _ = response.send(format!(
                    "{{\"ok\":false,\"error\":\"could not persist favorite: {}\"}}",
                    error.to_string().replace('"', "'")
                ));
                return;
            }
            (
                format!(
                    "favorite={} id={} name={}",
                    entry.candidate.favorite, entry.candidate.id, entry.candidate.name
                ),
                response,
            )
        }
        ControlCommand::HideCurrent(response) => {
            let Some(current) = tuner.current.as_ref().cloned() else {
                let _ = response.send("{\"ok\":false,\"error\":\"no current station\"}".into());
                return;
            };
            hidden_ids.insert(current.id.clone());
            if let Some(entry) = playable
                .iter_mut()
                .find(|entry| entry.candidate.id == current.id)
            {
                entry.candidate.hidden = true;
            }
            if let Err(error) = persist_hidden(config_path, &current.id, true) {
                let _ = response.send(format!(
                    "{{\"ok\":false,\"error\":\"could not persist hidden channel: {}\"}}",
                    error.to_string().replace('"', "'")
                ));
                return;
            }
            let action = tuner.next(
                &playable
                    .iter()
                    .map(|entry| entry.candidate.clone())
                    .collect::<Vec<_>>(),
                Instant::now(),
            );
            let (selection, response) = apply_control_selection(
                action,
                playable,
                selected_stream,
                service_status,
                response,
            );
            let hidden_action = format!("hidden=true id={}", current.id);
            let _ = response.send(format!(
                "{{\"ok\":true,\"action\":{hidden_action:?},\"selection\":{selection:?}}}"
            ));
            return;
        }
        ControlCommand::SetHidden {
            channel_id,
            hidden,
            response,
        } => {
            if hidden {
                hidden_ids.insert(channel_id.clone());
            } else {
                hidden_ids.remove(&channel_id);
            }
            if let Some(entry) = playable
                .iter_mut()
                .find(|entry| entry.candidate.id == channel_id)
            {
                entry.candidate.hidden = hidden;
            }
            if let Err(error) = persist_hidden(config_path, &channel_id, hidden) {
                let _ = response.send(format!(
                    "{{\"ok\":false,\"error\":\"could not persist hidden channel: {}\"}}",
                    error.to_string().replace('"', "'")
                ));
                return;
            }
            (format!("hidden={hidden} id={channel_id}"), response)
        }
        ControlCommand::SetLocation(location, response) => {
            *manual_location = location;
            *last_discovery = None;
            if let Ok(mut status) = service_status.lock() {
                status.location = location;
                status.confidence = if location.is_some() {
                    "ManualOverride".into()
                } else {
                    "Unknown".into()
                };
            }
            (format!("manual_location={location:?}"), response)
        }
    };
    let _ = response.send(format!("{{\"ok\":true,\"action\":{action:?}}}"));
}

fn persist_favorite(
    path: &Path,
    channel_id: &str,
    name: &str,
    country: Option<String>,
    favorite: bool,
) -> std::io::Result<()> {
    let mut app_config = config::load(path)?.config;
    app_config
        .favorites
        .retain(|saved| saved.channel_id != channel_id);
    if favorite {
        app_config.favorites.push(config::FavoriteSnapshot {
            channel_id: channel_id.to_owned(),
            name: name.to_owned(),
            country,
        });
    }
    config::store(path, &app_config)
}

fn persist_hidden(path: &Path, channel_id: &str, hidden: bool) -> std::io::Result<()> {
    let mut app_config = config::load(path)?.config;
    app_config
        .hidden_channels
        .retain(|saved| saved != channel_id);
    if hidden {
        app_config.hidden_channels.push(channel_id.to_owned());
    }
    config::store(path, &app_config)
}

fn apply_control_selection(
    action: TuneAction,
    playable: &[roadwaves_core::discovery::PlayableCandidate],
    selected_stream: &Mutex<Option<String>>,
    service_status: &Mutex<ServiceStatus>,
    response: Sender<String>,
) -> (String, Sender<String>) {
    if let TuneAction::Switched(candidate) = &action {
        if let Some(entry) = playable
            .iter()
            .find(|entry| entry.candidate.id == candidate.id)
        {
            if let Ok(mut current) = selected_stream.lock() {
                *current = Some(entry.stream.final_url.clone());
            }
            if let Ok(mut status) = service_status.lock() {
                status.current_channel_id = Some(candidate.id.clone());
                status.current_station = Some(candidate.name.clone());
                status.current_country = candidate.country.clone();
                status.current_city = candidate.city.clone();
                status.provider_error = None;
                status.provider = "Radio Garden".into();
            }
        }
    }
    (format!("{action:?}"), response)
}

#[derive(Debug, Serialize)]
struct DoctorReport {
    provider: &'static str,
    audio_endpoint: &'static str,
    telemetry_receiver: &'static str,
    profiles: Vec<roadwaves_core::GameInstall>,
    game_bins: Vec<PathBuf>,
    service_reachable: bool,
    service_error: Option<String>,
    radio_entries: Vec<PathBuf>,
    plugin_targets: Vec<PathBuf>,
}

fn doctor_report() -> DoctorReport {
    let profiles = discover_ets2_profiles();
    let game_bins = discover_ets2_game_bins();
    let (service_reachable, service_error) = service_health_check();
    let radio_entries = profiles
        .iter()
        .filter(|profile| has_roadwaves_radio_entry(&profile.radio_file))
        .map(|profile| profile.radio_file.clone())
        .collect();
    let plugin_targets = game_bins
        .iter()
        .filter(|game_bin| has_roadwaves_plugin(game_bin))
        .cloned()
        .collect();
    DoctorReport {
        provider: "Radio Garden only",
        audio_endpoint: "http://127.0.0.1:47926/radio.mp3",
        telemetry_receiver: "udp://127.0.0.1:47927",
        profiles,
        game_bins,
        service_reachable,
        service_error,
        radio_entries,
        plugin_targets,
    }
}

fn has_roadwaves_radio_entry(path: &std::path::Path) -> bool {
    std::fs::read_to_string(path)
        .map(|contents| contents.contains("|RoadWaves AutoTune|"))
        .unwrap_or(false)
}

fn has_roadwaves_plugin(game_bin: &std::path::Path) -> bool {
    let names: &[&str] = match game_bin
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("linux_x64") => &["roadwaves_scs.so"],
        Some("win_x64") => &["roadwaves_scs.dll"],
        _ => &["roadwaves_scs.dll", "roadwaves_scs.so"],
    };
    names
        .iter()
        .any(|name| game_bin.join("plugins").join(name).is_file())
}

fn service_health_check() -> (bool, Option<String>) {
    let address = match "127.0.0.1:47926".parse::<SocketAddr>() {
        Ok(address) => address,
        Err(error) => return (false, Some(error.to_string())),
    };
    let mut stream = match TcpStream::connect_timeout(&address, Duration::from_millis(500)) {
        Ok(stream) => stream,
        Err(error) => return (false, Some(error.to_string())),
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(750)));
    if let Err(error) = stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1:47926\r\nConnection: close\r\n\r\n")
    {
        return (false, Some(error.to_string()));
    }
    let mut response = String::new();
    if let Err(error) = stream.read_to_string(&mut response) {
        return (false, Some(error.to_string()));
    }
    if response.starts_with("HTTP/1.1 200") && response.contains("\"ok\":true") {
        (true, None)
    } else {
        (false, Some("service returned an unhealthy response".into()))
    }
}

#[derive(Debug, Deserialize)]
struct SimPoint {
    x: f64,
    y: f64,
    z: f64,
    #[serde(default)]
    paused: bool,
}

fn simulate(route: &PathBuf, speed: f64) -> Result<()> {
    if !speed.is_finite() || speed <= 0.0 {
        bail!("simulation speed must be finite and greater than zero");
    }
    let points: Vec<SimPoint> = serde_json::from_slice(
        &std::fs::read(route)
            .with_context(|| format!("could not read route {}", route.display()))?,
    )
    .with_context(|| {
        format!(
            "route {} must be a JSON array of x/y/z points",
            route.display()
        )
    })?;
    if points.is_empty() {
        bail!("route contains no telemetry points");
    }
    let socket = UdpSocket::bind("127.0.0.1:0")?;
    let delay = Duration::from_secs_f64((1.0 / speed).clamp(0.01, 5.0));
    for (sequence, point) in points.iter().enumerate() {
        if ![point.x, point.y, point.z]
            .iter()
            .all(|value| value.is_finite())
        {
            bail!("route point {sequence} contains a non-finite coordinate");
        }
        let packet = serde_json::json!({
            "protocol_version": 1,
            "game": "ets2",
            "session_id": "synthetic-route",
            "sequence": sequence as u64,
            "timestamp_unix_ms": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
            "world": {"x": point.x, "y": point.y, "z": point.z},
            "heading": null,
            "paused": point.paused
        });
        socket.send_to(packet.to_string().as_bytes(), "127.0.0.1:47927")?;
        println!("simulated point {}/{}", sequence + 1, points.len());
        thread::sleep(delay);
    }
    Ok(())
}

fn calibrate(anchors_path: &PathBuf, output: Option<&std::path::Path>) -> Result<()> {
    let anchors: Vec<WorldAnchor> = serde_json::from_slice(
        &std::fs::read(anchors_path)
            .with_context(|| format!("could not read anchors {}", anchors_path.display()))?,
    )
    .with_context(|| "anchors must be a JSON array of exactly three world_x/world_z/real points")?;
    let calibration = Calibration::from_anchors(&anchors)
        .context("calibration requires exactly three valid, non-collinear anchors")?;
    let destination = output
        .map(PathBuf::from)
        .unwrap_or_else(|| roadwaves_core::cache::user_cache_dir().join("calibration.json"));
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = destination.with_extension("tmp");
    std::fs::write(&temporary, serde_json::to_vec_pretty(&calibration)?)?;
    std::fs::rename(&temporary, &destination)?;
    println!("saved manual calibration to {}", destination.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        has_roadwaves_plugin, header_value, loopback_bind_address, persist_favorite,
        persist_hidden, request_discovery_refresh, ControlCommand,
    };
    use roadwaves_core::config;
    use std::{
        fs,
        net::{IpAddr, Ipv4Addr, Ipv6Addr},
        sync::mpsc,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn accepts_only_numeric_loopback_addresses() {
        assert_eq!(
            loopback_bind_address("127.0.0.1:47926").unwrap().ip(),
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        );
        assert_eq!(
            loopback_bind_address("[::1]:47926").unwrap().ip(),
            IpAddr::V6(Ipv6Addr::LOCALHOST)
        );
        assert!(loopback_bind_address("0.0.0.0:47926").is_err());
        assert!(loopback_bind_address("localhost:47926").is_err());
    }

    #[test]
    fn http_header_names_are_case_insensitive() {
        assert_eq!(
            header_value("x-roadwaves-token: secret", "X-RoadWaves-Token"),
            Some("secret")
        );
        assert_eq!(
            header_value("CONTENT-LENGTH: 42", "Content-Length"),
            Some("42")
        );
    }

    #[test]
    fn service_favorite_persistence_round_trips_add_and_remove() {
        let directory = std::env::temp_dir().join(format!(
            "roadwaves-cli-favorite-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let path = directory.join("config.json");
        persist_favorite(&path, "station-id", "Station", Some("DE".into()), true).unwrap();
        let loaded = config::load(&path).unwrap().config;
        assert_eq!(loaded.favorites.len(), 1);
        assert_eq!(loaded.favorites[0].channel_id, "station-id");
        persist_favorite(&path, "station-id", "Station", Some("DE".into()), false).unwrap();
        assert!(config::load(&path).unwrap().config.favorites.is_empty());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn service_hidden_persistence_round_trips_add_and_remove() {
        let directory = std::env::temp_dir().join(format!(
            "roadwaves-cli-hidden-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let path = directory.join("config.json");
        persist_hidden(&path, "station-id", true).unwrap();
        assert_eq!(
            config::load(&path).unwrap().config.hidden_channels,
            ["station-id"]
        );
        persist_hidden(&path, "station-id", false).unwrap();
        assert!(config::load(&path)
            .unwrap()
            .config
            .hidden_channels
            .is_empty());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn doctor_plugin_state_matches_target_format() {
        let root = std::env::temp_dir().join(format!(
            "roadwaves-cli-doctor-plugin-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let linux_bin = root.join("bin/linux_x64/plugins");
        fs::create_dir_all(&linux_bin).unwrap();
        fs::write(linux_bin.join("roadwaves_scs.dll"), b"wrong format").unwrap();
        assert!(!has_roadwaves_plugin(&root.join("bin/linux_x64")));
        fs::write(linux_bin.join("roadwaves_scs.so"), b"correct format").unwrap();
        assert!(has_roadwaves_plugin(&root.join("bin/linux_x64")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn failed_audio_requests_signal_bounded_discovery_refresh() {
        let (sender, receiver) = mpsc::channel();
        request_discovery_refresh(Some(&sender));
        assert!(matches!(
            receiver.try_recv(),
            Ok(ControlCommand::Refresh(_))
        ));
    }
}
