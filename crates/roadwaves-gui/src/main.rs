use eframe::egui;
use egui::{Color32, RichText, Stroke};
use roadwaves_core::{
    cache::user_cache_dir,
    config::{self, AppConfig},
    discover_ets2_game_bins, discover_ets2_profiles, GeoPoint, RadioGardenClient, SiiEditor,
    Station, TuneMode,
};
use serde_json::Value;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

type ProviderRefresh = Result<(Vec<Station>, bool, bool), String>;

struct RoadWavesApp {
    profiles: Vec<roadwaves_core::GameInstall>,
    game_bins: Vec<std::path::PathBuf>,
    autotune: roadwaves_core::AutoTune,
    service: Option<Child>,
    status: String,
    provider_status: String,
    provider_rx: Option<Receiver<ProviderRefresh>>,
    provider_busy: bool,
    places: Vec<Station>,
    control_token: Option<String>,
    control_rx: Option<Receiver<Result<String, String>>>,
    config: AppConfig,
    config_path: std::path::PathBuf,
    log_path: std::path::PathBuf,
    service_status: String,
    now_playing: String,
    station_name: String,
    station_city: String,
    station_country: String,
    station_mode: String,
    station_confidence: String,
    status_rx: Option<Receiver<Result<String, String>>>,
    last_status_poll: Instant,
    manual_latitude: String,
    manual_longitude: String,
    selected_profile: usize,
    selected_game_bin: usize,
    legacy_layout: bool,
}

impl Default for RoadWavesApp {
    fn default() -> Self {
        let config_path = config::user_config_path();
        let log_path = user_cache_dir().join("roadwaves.log");
        let loaded = config::load(&config_path).unwrap_or_else(|error| config::ConfigLoad {
            config: AppConfig::default(),
            warning: Some(format!("Could not read settings: {error}")),
        });
        let mut autotune = roadwaves_core::AutoTune::default();
        autotune.set_mode(loaded.config.mode);
        Self {
            profiles: discover_ets2_profiles(),
            game_bins: discover_ets2_game_bins(),
            autotune,
            service: None,
            status: loaded
                .warning
                .unwrap_or_else(|| "Live game files are unchanged.".into()),
            provider_status: "Not queried".into(),
            provider_rx: None,
            provider_busy: false,
            places: Vec::new(),
            control_token: None,
            control_rx: None,
            config: loaded.config.clone(),
            config_path,
            log_path,
            service_status: "Awaiting local service.".into(),
            now_playing: "No active station.".into(),
            station_name: "No active station".into(),
            station_city: "Waiting for ETS2 telemetry".into(),
            station_country: String::new(),
            station_mode: "AutoTune".into(),
            station_confidence: "Unknown".into(),
            status_rx: None,
            last_status_poll: Instant::now() - Duration::from_secs(2),
            manual_latitude: loaded
                .config
                .manual_location
                .map(|location| format!("{:.6}", location.latitude))
                .unwrap_or_default(),
            manual_longitude: loaded
                .config
                .manual_location
                .map(|location| format!("{:.6}", location.longitude))
                .unwrap_or_default(),
            selected_profile: 0,
            selected_game_bin: 0,
            legacy_layout: false,
        }
    }
}

impl eframe::App for RoadWavesApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(receiver) = &self.control_rx {
            match receiver.try_recv() {
                Ok(Ok(response)) => {
                    if response.contains("mode=AutoTune") {
                        self.autotune.set_mode(TuneMode::AutoTune);
                    } else if response.contains("mode=Explore") {
                        self.autotune.set_mode(TuneMode::Explore);
                    } else if response.contains("mode=FavoriteLock") {
                        self.autotune.set_mode(TuneMode::FavoriteLock);
                    }
                    if let Some(favorite) = parse_favorite_response(&response) {
                        if favorite.2 {
                            self.config
                                .favorites
                                .retain(|item| item.channel_id != favorite.0);
                            self.config
                                .favorites
                                .push(roadwaves_core::config::FavoriteSnapshot {
                                    channel_id: favorite.0,
                                    name: favorite.1,
                                    country: None,
                                });
                        } else {
                            self.config
                                .favorites
                                .retain(|item| item.channel_id != favorite.0);
                        }
                    }
                    if let Some((channel_id, hidden)) = parse_hidden_response(&response) {
                        if hidden {
                            if !self.config.hidden_channels.contains(&channel_id) {
                                self.config.hidden_channels.push(channel_id);
                            }
                        } else {
                            self.config.hidden_channels.retain(|id| id != &channel_id);
                        }
                    }
                    self.config.mode = self.autotune.mode;
                    self.persist_config();
                    self.status = format!("Service control acknowledged: {response}");
                    self.control_rx = None;
                }
                Ok(Err(error)) => {
                    self.status = format!("Service control failed: {error}");
                    self.control_rx = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.status = "Service control worker stopped unexpectedly.".into();
                    self.control_rx = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
            context.request_repaint_after(Duration::from_millis(100));
        }
        if let Some(receiver) = &self.status_rx {
            match receiver.try_recv() {
                Ok(Ok(status)) => {
                    let snapshot = runtime_snapshot(&status);
                    self.station_name = snapshot.station;
                    self.station_city = snapshot.city;
                    self.station_country = snapshot.country;
                    self.station_mode = snapshot.mode;
                    self.station_confidence = snapshot.confidence;
                    self.now_playing = summarize_runtime_status(&status);
                    self.service_status = status;
                    self.status_rx = None;
                }
                Ok(Err(error)) => {
                    self.service_status = format!("status unavailable: {error}");
                    self.status_rx = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.service_status = "status worker stopped unexpectedly".into();
                    self.status_rx = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
            context.request_repaint_after(Duration::from_millis(100));
        }
        if let Some(receiver) = &self.provider_rx {
            match receiver.try_recv() {
                Ok(Ok((places, from_cache, stale))) => {
                    self.places = places;
                    self.provider_busy = false;
                    self.provider_rx = None;
                    self.provider_status = if from_cache {
                        if stale {
                            "Cached Radio Garden metadata (stale); live streams still require provider access.".into()
                        } else {
                            "Cached Radio Garden metadata; live streams still require provider access.".into()
                        }
                    } else {
                        "Radio Garden metadata is live.".into()
                    };
                    self.status = format!("Loaded {} Radio Garden places.", self.places.len());
                }
                Ok(Err(error)) => {
                    self.provider_busy = false;
                    self.provider_rx = None;
                    self.provider_status = format!("Unavailable: {error}");
                    self.status =
                        "Radio Garden is unavailable; no alternate provider is used.".into();
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.provider_busy = false;
                    self.provider_rx = None;
                    self.provider_status =
                        "Unavailable: refresh worker stopped unexpectedly.".into();
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
            context.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if self
            .service
            .as_mut()
            .is_some_and(|child| child.try_wait().ok().flatten().is_some())
        {
            self.service = None;
            self.control_token = None;
            self.status_rx = None;
            self.service_status = "Local service stopped.".into();
            self.now_playing = "No active station.".into();
        }
        if self.service.is_some()
            && self.status_rx.is_none()
            && self.last_status_poll.elapsed() >= Duration::from_secs(1)
        {
            self.last_status_poll = Instant::now();
            let (sender, receiver) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = sender.send(fetch_service_status());
            });
            self.status_rx = Some(receiver);
        }
        if self.legacy_layout {
            egui::CentralPanel::default().show(context, |ui| {
            ui.heading("RoadWaves");
            ui.label("Drive Europe. Hear local radio.");
            ui.separator();
            ui.heading("ETS2 integration");
            if self.profiles.is_empty() { ui.colored_label(egui::Color32::YELLOW, "No ETS2 radio profiles detected."); }
            if !self.profiles.is_empty() {
                self.selected_profile = self.selected_profile.min(self.profiles.len() - 1);
                egui::ComboBox::from_label("Radio profile")
                    .selected_text(format!("{} — {}", self.profiles[self.selected_profile].mode, self.profiles[self.selected_profile].radio_file.display()))
                    .show_ui(ui, |ui| {
                        for (index, profile) in self.profiles.iter().enumerate() {
                            ui.selectable_value(
                                &mut self.selected_profile,
                                index,
                                format!("{} — {}", profile.mode, profile.radio_file.display()),
                            );
                        }
                    });
            }
            if !self.game_bins.is_empty() {
                self.selected_game_bin = self.selected_game_bin.min(self.game_bins.len() - 1);
                egui::ComboBox::from_label("Game binary")
                    .selected_text(self.game_bins[self.selected_game_bin].display().to_string())
                    .show_ui(ui, |ui| {
                        for (index, path) in self.game_bins.iter().enumerate() {
                            ui.selectable_value(&mut self.selected_game_bin, index, path.display().to_string());
                        }
                    });
            }
            ui.horizontal(|ui| {
                if ui.button("Refresh profiles").clicked() { self.profiles = discover_ets2_profiles(); self.game_bins = discover_ets2_game_bins(); self.status = "Profile discovery refreshed; no files changed.".into(); }
                if ui.button("Install / Repair Integration").clicked() {
                    let source = bundled_plugin_path();
                    match (source, self.profiles.get(self.selected_profile), self.game_bins.get(self.selected_game_bin)) {
                        (Some(source), Some(profile), Some(game_bin)) => match SiiEditor::install_integration(&source, game_bin, &profile.radio_file, "http://127.0.0.1:47926/radio.mp3") { Ok(_) => self.status = "Integration installed with backups; select RoadWaves AutoTune in ETS2.".into(), Err(error) => self.status = format!("Integration failed without claiming success: {error}"), },
                        _ => self.status = "Integration needs a detected ETS2 profile, game binary, and packaged plugin.".into(),
                    }
                }
                if ui.button("Uninstall Integration").clicked() {
                    match (self.profiles.get(self.selected_profile), self.game_bins.get(self.selected_game_bin)) {
                        (Some(profile), Some(game_bin)) => {
                            match SiiEditor::uninstall_integration(&profile.radio_file, game_bin) {
                                Ok((radio, plugin)) => {
                                    self.status = format!("Uninstall complete: {}; {}. Backups were retained.", radio.message, plugin.message)
                                }
                                Err(error) => self.status = format!("Uninstall failed without claiming success: {error}"),
                            }
                        }
                        _ => self.status = "Uninstall needs a selected ETS2 profile and game binary.".into(),
                    }
                }
                if ui.button("AutoTune").clicked() { self.request_control(r#"{"action":"mode","mode":"AutoTune"}"#, "AutoTune"); }
                if ui.button("Explore").clicked() { self.request_control(r#"{"action":"mode","mode":"Explore"}"#, "Explore"); }
                let lock_label = if self.autotune.state.favorite_lock { "Unlock Favorite" } else { "Favorite Lock" };
                if ui.button(lock_label).clicked() {
                    let mode = if self.autotune.state.favorite_lock { "AutoTune" } else { "FavoriteLock" };
                    self.request_control(&format!(r#"{{"action":"mode","mode":"{mode}"}}"#), "Favorite Lock");
                }
                if ui.button("Next").clicked() { self.request_control(r#"{"action":"next"}"#, "Next"); }
                if ui.button("Previous").clicked() { self.request_control(r#"{"action":"previous"}"#, "Previous"); }
                if ui.button("Favorite").clicked() { self.request_control(r#"{"action":"favorite"}"#, "Favorite"); }
                if ui.button("Hide current").clicked() { self.request_control(r#"{"action":"hide"}"#, "Hide current"); }
            });
            ui.separator();
            ui.heading("Service");
            ui.label("Audio: http://127.0.0.1:47926/radio.mp3");
            ui.label("Telemetry: udp://127.0.0.1:47927");
            ui.label(format!("Now playing: {}", self.now_playing));
            ui.label(format!("Runtime status: {}", self.service_status));
            ui.collapsing("Manual location override", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Latitude");
                    ui.text_edit_singleline(&mut self.manual_latitude);
                    ui.label("Longitude");
                    ui.text_edit_singleline(&mut self.manual_longitude);
                });
                ui.horizontal(|ui| {
                    if ui.button("Apply location").clicked() {
                        match (
                            self.manual_latitude.trim().parse::<f64>(),
                            self.manual_longitude.trim().parse::<f64>(),
                        ) {
                            (Ok(latitude), Ok(longitude)) => {
                                let location = GeoPoint { latitude, longitude };
                                if location.valid() {
                                    self.config.manual_location = Some(location);
                                    self.persist_config();
                                    self.request_control(
                                        &format!(
                                            r#"{{"action":"location","latitude":{latitude},"longitude":{longitude}}}"#
                                        ),
                                        "Apply manual location",
                                    );
                                } else {
                                    self.status = "Manual location must use valid latitude/longitude ranges.".into();
                                }
                            }
                            _ => self.status = "Manual location requires numeric latitude and longitude.".into(),
                        }
                    }
                    if ui.button("Clear location").clicked() {
                        self.config.manual_location = None;
                        self.manual_latitude.clear();
                        self.manual_longitude.clear();
                        self.persist_config();
                        self.request_control(r#"{"action":"clear_location"}"#, "Clear manual location");
                    }
                });
                ui.small("Manual location overrides calibrated telemetry for station selection; it does not claim GPS accuracy.");
            });
            ui.collapsing("Settings and diagnostics", |ui| {
                let mut start_with_login = self.config.start_with_login;
                if ui.checkbox(&mut start_with_login, "Start service preference (launcher startup)").changed() {
                    self.config.start_with_login = start_with_login;
                    self.persist_config();
                }
                ui.label(format!("Settings: {}", self.config_path.display()));
                ui.label(format!("Service log: {}", self.log_path.display()));
                ui.label(format!("Audio port: {} | Telemetry port: {}", self.config.audio_port, self.config.telemetry_port));
                ui.label(format!("Favorites saved: {} | Hidden channels: {}", self.config.favorites.len(), self.config.hidden_channels.len()));
                if self.config.favorites.is_empty() {
                    ui.small("No saved favorites yet. Use Favorite while a live station is selected.");
                } else {
                    ui.label("Saved favorites:");
                    for favorite in &self.config.favorites {
                        ui.label(format!(
                            "  {} [{}]{}",
                            favorite.name,
                            favorite.channel_id,
                            favorite
                                .country
                                .as_deref()
                                .map(|country| format!(" ({country})"))
                                .unwrap_or_default()
                        ));
                    }
                }
                if !self.config.hidden_channels.is_empty() {
                    ui.label("Hidden channels:");
                    for channel_id in self.config.hidden_channels.clone() {
                        ui.horizontal(|ui| {
                            ui.label(&channel_id);
                            if ui.button("Unhide").clicked() {
                                self.request_control(
                                    &format!(
                                        r#"{{"action":"unhide","channel_id":{channel_id:?}}}"#
                                    ),
                                    "Unhide channel",
                                );
                            }
                        });
                    }
                }
                ui.label(format!("Detected profiles: {} | game binaries: {}", self.profiles.len(), self.game_bins.len()));
                if let Some(profile) = self.profiles.get(self.selected_profile) {
                    ui.label(format!("Selected radio file: {}", profile.radio_file.display()));
                }
                if let Some(game_bin) = self.game_bins.get(self.selected_game_bin) {
                    ui.label(format!("Selected plugin target: {}", game_bin.display()));
                }
                ui.small("Diagnostics are local-only. Radio Garden metadata may be cached, but live audio still requires provider access.");
            });
            ui.horizontal(|ui| {
                if self.service.is_some() {
                    if ui.button("Stop service").clicked() {
                        if let Some(mut child) = self.service.take() { let _ = child.kill(); let _ = child.wait(); }
                        self.control_token = None;
                        self.status_rx = None;
                        self.service_status = "Local service stopped.".into();
                        self.now_playing = "No active station.".into();
                        self.status = "Local service stopped.".into();
                    }
                } else if ui.button("Start service").clicked() {
                    self.start_service();
                }
            });
            ui.separator();
            ui.heading("Radio Garden catalog");
            ui.label(format!("Provider: {}", self.provider_status));
            if self.provider_busy {
                ui.label("Refreshing metadata in the background…");
            } else if ui.button("Refresh local stations").clicked() {
                self.start_provider_refresh();
            }
            if self.places.is_empty() {
                ui.label("No catalog places loaded. Refresh to query Radio Garden or use cached metadata.");
            } else {
                for place in self.places.iter().take(8) {
                    ui.label(format!(
                        "{} — {} ({:.3}, {:.3})",
                        place.name,
                        place.country.as_deref().unwrap_or("country unknown"),
                        place.location.latitude,
                        place.location.longitude
                    ));
                }
                if self.places.len() > 8 {
                    ui.small(format!("{} more places available through the CLI catalog view.", self.places.len() - 8));
                }
            }
            ui.separator();
            ui.label(&self.status);
            ui.small("Real ETS2 playback and geographic accuracy still require manual game verification.");
            });
        }
        self.render_dashboard(context);
    }
}

impl RoadWavesApp {
    fn render_dashboard(&mut self, context: &egui::Context) {
        apply_dashboard_theme(context);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(10, 14, 22))
                    .inner_margin(24.0),
            )
            .show(context, |ui| {
                ui.set_max_width(940.0);

                ui.horizontal(|ui| {
                    ui.add_sized(
                        [48.0, 48.0],
                        egui::Button::new(
                            RichText::new("◉")
                                .size(28.0)
                                .color(Color32::from_rgb(74, 224, 212)),
                        )
                        .fill(Color32::from_rgb(32, 65, 76)),
                    );
                    ui.vertical(|ui| {
                        ui.label(RichText::new("roadwaves").size(25.0).strong());
                        ui.label(
                            RichText::new("Your world. Your radio.")
                                .color(Color32::from_rgb(151, 164, 185)),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (label, color) = if self.service.is_some() {
                            ("● Live", Color32::from_rgb(26, 195, 107))
                        } else {
                            ("○ Offline", Color32::from_rgb(145, 158, 180))
                        };
                        ui.label(RichText::new(label).strong().color(color));
                    });
                });
                ui.add_space(20.0);
                ui.separator();
                ui.add_space(18.0);

                let hero = egui::Frame::none()
                    .fill(Color32::from_rgb(27, 38, 52))
                    .rounding(18.0)
                    .inner_margin(20.0);
                hero.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            [104.0, 104.0],
                            egui::Button::new(
                                RichText::new("◉")
                                    .size(54.0)
                                    .color(Color32::from_rgb(79, 226, 213)),
                            )
                            .fill(Color32::from_rgb(42, 76, 87)),
                        );
                        ui.add_space(16.0);
                        ui.vertical(|ui| {
                            let country = if self.station_country.is_empty() {
                                "WAITING FOR LOCATION"
                            } else {
                                &self.station_country.to_uppercase()
                            };
                            ui.label(
                                RichText::new(format!("NOW PLAYING · {country}"))
                                    .size(14.0)
                                    .strong()
                                    .color(Color32::from_rgb(75, 225, 211)),
                            );
                            ui.label(RichText::new(&self.station_name).size(29.0).strong());
                            let place = if self.station_country.is_empty() {
                                self.station_city.clone()
                            } else {
                                format!("{}, {}", self.station_city, self.station_country)
                            };
                            ui.label(
                                RichText::new(place)
                                    .size(16.0)
                                    .color(Color32::from_rgb(163, 177, 197)),
                            );
                        });
                    });
                    ui.add_space(18.0);
                    ui.horizontal_centered(|ui| {
                        if ui
                            .add_sized(
                                [44.0, 34.0],
                                egui::Button::new(RichText::new("|‹").size(22.0)),
                            )
                            .clicked()
                        {
                            self.request_control(r#"{"action":"previous"}"#, "Previous");
                        }
                        if ui
                            .add_sized(
                                [52.0, 52.0],
                                egui::Button::new(RichText::new("Ⅱ").size(20.0))
                                    .fill(Color32::from_rgb(242, 244, 247)),
                            )
                            .clicked()
                        {
                            self.status =
                                "Playback is controlled by ETS2's native radio player.".into();
                        }
                        if ui
                            .add_sized(
                                [44.0, 34.0],
                                egui::Button::new(RichText::new("›|").size(22.0)),
                            )
                            .clicked()
                        {
                            self.request_control(r#"{"action":"next"}"#, "Next");
                        }
                    });
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("◖)")
                                .size(18.0)
                                .color(Color32::from_rgb(195, 207, 222)),
                        );
                        ui.add(
                            egui::ProgressBar::new(0.74)
                                .desired_width(ui.available_width())
                                .text(""),
                        );
                    });
                });

                ui.add_space(16.0);
                ui.columns(2, |columns| {
                    info_card(
                        &mut columns[0],
                        "⌖",
                        "Auto discovery",
                        "Find radio by location",
                        Color32::from_rgb(74, 224, 212),
                    );
                    info_card(
                        &mut columns[1],
                        "▣",
                        "Game overlay",
                        "Ctrl + Alt + R",
                        Color32::from_rgb(74, 224, 212),
                    );
                });

                ui.add_space(16.0);
                let panel = egui::Frame::none()
                    .fill(Color32::from_rgb(17, 24, 35))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(43, 57, 75)))
                    .rounding(14.0)
                    .inner_margin(16.0);
                panel.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("CONTROL CENTER")
                                .size(12.0)
                                .strong()
                                .color(Color32::from_rgb(127, 145, 168)),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if self.service.is_some() {
                                if ui.button("Stop service").clicked() {
                                    if let Some(mut child) = self.service.take() {
                                        let _ = child.kill();
                                        let _ = child.wait();
                                    }
                                    self.control_token = None;
                                    self.status_rx = None;
                                    self.service_status = "Local service stopped.".into();
                                    self.station_name = "No active station".into();
                                    self.station_city =
                                        "Start the service to tune automatically".into();
                                }
                            } else if ui.button("Start service").clicked() {
                                self.start_service();
                            }
                        });
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label("Mode");
                        for (mode, label) in [
                            (TuneMode::AutoTune, "AutoTune"),
                            (TuneMode::Explore, "Explore"),
                        ] {
                            let selected = self.autotune.mode == mode;
                            if ui
                                .add(egui::SelectableLabel::new(selected, label))
                                .clicked()
                            {
                                self.request_control(
                                    &format!(r#"{{"action":"mode","mode":"{label}"}}"#),
                                    label,
                                );
                            }
                        }
                        let locked = self.autotune.state.favorite_lock;
                        if ui
                            .add(egui::SelectableLabel::new(locked, "Favorite Lock"))
                            .clicked()
                        {
                            let mode = if locked { "AutoTune" } else { "FavoriteLock" };
                            self.request_control(
                                &format!(r#"{{"action":"mode","mode":"{mode}"}}"#),
                                "Favorite Lock",
                            );
                        }
                        ui.separator();
                        if ui.button("♡ Favorite").clicked() {
                            self.request_control(r#"{"action":"favorite"}"#, "Favorite");
                        }
                        if ui.button("Hide").clicked() {
                            self.request_control(r#"{"action":"hide"}"#, "Hide current");
                        }
                    });
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(format!(
                            "{} · {} · confidence {}",
                            self.station_mode, self.provider_status, self.station_confidence
                        ))
                        .small()
                        .color(Color32::from_rgb(140, 155, 178)),
                    );
                });

                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    if ui.button("Install / Repair Integration").clicked() {
                        self.install_integration();
                    }
                    if ui.button("Refresh profiles").clicked() {
                        self.profiles = discover_ets2_profiles();
                        self.game_bins = discover_ets2_game_bins();
                        self.status = "Profiles refreshed; no files changed.".into();
                    }
                    if ui.button("Refresh stations").clicked() {
                        self.start_provider_refresh();
                    }
                });
                egui::CollapsingHeader::new("Settings & diagnostics")
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.label(&self.status);
                        ui.label(format!("Service: {}", self.service_status));
                        ui.label(format!(
                            "Detected profiles: {} · game binaries: {}",
                            self.profiles.len(),
                            self.game_bins.len()
                        ));
                        ui.label(format!("Log: {}", self.log_path.display()));
                        ui.horizontal(|ui| {
                            ui.label("Manual location");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.manual_latitude)
                                    .hint_text("latitude"),
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut self.manual_longitude)
                                    .hint_text("longitude"),
                            );
                            if ui.button("Apply").clicked() {
                                self.apply_manual_location();
                            }
                            if ui.button("Clear").clicked() {
                                self.clear_manual_location();
                            }
                        });
                    });
                ui.add_space(16.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.hyperlink_to(
                        "GitHub · Source code ↗",
                        "https://github.com/tomas-santucho/roadwaves",
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new("Open source · Radio Garden provider")
                                .color(Color32::from_rgb(125, 141, 164)),
                        );
                    });
                });
            });
    }

    fn install_integration(&mut self) {
        let source = bundled_plugin_path();
        match (
            source,
            self.profiles.get(self.selected_profile),
            self.game_bins.get(self.selected_game_bin),
        ) {
            (Some(source), Some(profile), Some(game_bin)) => match SiiEditor::install_integration(
                &source,
                game_bin,
                &profile.radio_file,
                "http://127.0.0.1:47926/radio.mp3",
            ) {
                Ok(_) => {
                    self.status =
                        "Integration installed with backups. Select RoadWaves AutoTune in ETS2."
                            .into()
                }
                Err(error) => {
                    self.status = format!("Integration failed without claiming success: {error}")
                }
            },
            _ => {
                self.status =
                    "Integration needs a detected ETS2 profile, game binary, and packaged plugin."
                        .into()
            }
        }
    }

    fn apply_manual_location(&mut self) {
        match (
            self.manual_latitude.trim().parse::<f64>(),
            self.manual_longitude.trim().parse::<f64>(),
        ) {
            (Ok(latitude), Ok(longitude))
                if (GeoPoint {
                    latitude,
                    longitude,
                })
                .valid() =>
            {
                self.config.manual_location = Some(GeoPoint {
                    latitude,
                    longitude,
                });
                self.persist_config();
                self.request_control(
                    &format!(
                        r#"{{"action":"location","latitude":{latitude},"longitude":{longitude}}}"#
                    ),
                    "Apply manual location",
                );
            }
            _ => self.status = "Manual location needs valid numeric latitude and longitude.".into(),
        }
    }

    fn clear_manual_location(&mut self) {
        self.config.manual_location = None;
        self.manual_latitude.clear();
        self.manual_longitude.clear();
        self.persist_config();
        self.request_control(r#"{"action":"clear_location"}"#, "Clear manual location");
    }

    fn start_service(&mut self) {
        let token = match control_token() {
            Ok(token) => token,
            Err(error) => {
                self.status = format!("Could not create authenticated service token: {error}");
                return;
            }
        };
        let result = std::env::current_exe().and_then(|path| {
            let cli = path.with_file_name(if cfg!(windows) {
                "roadwaves.exe"
            } else {
                "roadwaves"
            });
            if let Some(parent) = self.log_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let log = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.log_path)?;
            let error_log = log.try_clone()?;
            Command::new(cli)
                .args(["serve", "--control-token", &token])
                .stdout(Stdio::from(log))
                .stderr(Stdio::from(error_log))
                .spawn()
        });
        match result {
            Ok(child) => {
                self.service = Some(child);
                self.control_token = Some(token);
                self.status =
                    "Local audio and telemetry service started with authenticated controls.".into();
                let mode = match self.config.mode {
                    TuneMode::AutoTune => "AutoTune",
                    TuneMode::Explore => "Explore",
                    TuneMode::FavoriteLock => "FavoriteLock",
                };
                self.request_control(
                    &format!(r#"{{"action":"mode","mode":"{mode}"}}"#),
                    "Restore saved mode",
                );
            }
            Err(error) => self.status = format!("Could not start local service: {error}"),
        }
    }

    fn persist_config(&mut self) {
        if let Err(error) = config::store(&self.config_path, &self.config) {
            self.status = format!("Could not persist settings: {error}");
        }
    }

    fn request_control(&mut self, body: &str, label: &str) {
        if self.control_rx.is_some() {
            self.status = "Another service control request is still pending.".into();
            return;
        }
        let Some(token) = self.control_token.clone() else {
            self.status = format!("{label} requires the RoadWaves service to be running.");
            return;
        };
        let body = body.to_owned();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let result = send_control_request(&token, &body);
            let _ = sender.send(result);
        });
        self.control_rx = Some(receiver);
        self.status = format!("Sending {label} command to the local service…");
    }

    fn start_provider_refresh(&mut self) {
        if self.provider_busy {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let cache_path = user_cache_dir().join("places.json");
        std::thread::spawn(move || {
            let result = RadioGardenClient::default()
                .places_with_cache(&cache_path)
                .map(|cached| (cached.places, cached.from_cache, cached.stale))
                .map_err(|error| error.to_string());
            let _ = sender.send(result);
        });
        self.provider_busy = true;
        self.provider_rx = Some(receiver);
        self.provider_status = "Checking Radio Garden…".into();
    }
}

fn control_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

struct RuntimeSnapshot {
    station: String,
    city: String,
    country: String,
    mode: String,
    confidence: String,
}

fn runtime_snapshot(raw: &str) -> RuntimeSnapshot {
    let value = serde_json::from_str::<Value>(raw).unwrap_or_default();
    let text = |key: &str, fallback: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or(fallback)
            .to_owned()
    };
    RuntimeSnapshot {
        station: text("current_station", "No active station"),
        city: text("current_city", "Waiting for ETS2 telemetry"),
        country: text("current_country", ""),
        mode: text("mode", "AutoTune"),
        confidence: text("confidence", "Unknown"),
    }
}

fn apply_dashboard_theme(context: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(Color32::from_rgb(232, 237, 244));
    visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(17, 24, 35);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(27, 38, 52);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(39, 57, 72);
    visuals.widgets.active.bg_fill = Color32::from_rgb(47, 74, 84);
    visuals.selection.bg_fill = Color32::from_rgb(47, 127, 132);
    context.set_visuals(visuals);
}

fn info_card(ui: &mut egui::Ui, icon: &str, title: &str, subtitle: &str, accent: Color32) {
    egui::Frame::none()
        .fill(Color32::from_rgb(27, 38, 52))
        .rounding(13.0)
        .inner_margin(16.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon).size(24.0).color(accent));
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).size(18.0).strong());
                    ui.label(RichText::new(subtitle).color(Color32::from_rgb(157, 173, 195)));
                });
            });
        });
}

fn parse_favorite_response(response: &str) -> Option<(String, String, bool)> {
    let marker = response.find("favorite=")?;
    let rest = &response[marker + "favorite=".len()..];
    let favorite = rest
        .strip_prefix("true")
        .map(|_| true)
        .or_else(|| rest.strip_prefix("false").map(|_| false))?;
    let id = rest
        .split(" id=")
        .nth(1)?
        .split(" name=")
        .next()?
        .to_owned();
    let name = rest
        .split(" name=")
        .nth(1)?
        .trim_end_matches('}')
        .trim_end_matches('"')
        .to_owned();
    Some((id, name, favorite))
}

fn parse_hidden_response(response: &str) -> Option<(String, bool)> {
    let marker = response.find("hidden=")?;
    let rest = &response[marker + "hidden=".len()..];
    let hidden = rest
        .strip_prefix("true")
        .map(|_| true)
        .or_else(|| rest.strip_prefix("false").map(|_| false))?;
    let id = rest
        .split(" id=")
        .nth(1)?
        .split_whitespace()
        .next()?
        .to_owned();
    Some((id, hidden))
}

fn send_control_request(token: &str, body: &str) -> Result<String, String> {
    let address = "127.0.0.1:47926"
        .parse::<SocketAddr>()
        .map_err(|error| error.to_string())?;
    let mut stream = None;
    let mut last_error = String::from("service listener did not become ready");
    for _ in 0..20 {
        match TcpStream::connect_timeout(&address, Duration::from_millis(250)) {
            Ok(connection) => {
                stream = Some(connection);
                break;
            }
            Err(error) => {
                last_error = error.to_string();
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
    let mut stream = stream.ok_or(last_error)?;
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|error| error.to_string())?;
    let request = format!(
        "POST /control HTTP/1.1\r\nHost: 127.0.0.1:47926\r\nX-RoadWaves-Token: {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| error.to_string())?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| error.to_string())?;
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| "invalid service response".to_owned())?;
    if !headers.starts_with("HTTP/1.1 200") {
        return Err(format!(
            "service returned {}",
            headers.lines().next().unwrap_or("error")
        ));
    }
    Ok(body.to_owned())
}

fn fetch_service_status() -> Result<String, String> {
    let mut stream = TcpStream::connect_timeout(
        &"127.0.0.1:47926"
            .parse::<SocketAddr>()
            .map_err(|error| error.to_string())?,
        Duration::from_secs(2),
    )
    .map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|error| error.to_string())?;
    stream
        .write_all(b"GET /status HTTP/1.1\r\nHost: 127.0.0.1:47926\r\nConnection: close\r\n\r\n")
        .map_err(|error| error.to_string())?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| error.to_string())?;
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| "invalid service response".to_owned())?;
    if !headers.starts_with("HTTP/1.1 200") {
        return Err(format!(
            "service returned {}",
            headers.lines().next().unwrap_or("error")
        ));
    }
    Ok(body.to_owned())
}

fn summarize_runtime_status(raw: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return raw.to_owned();
    };
    let station = value
        .get("current_station")
        .and_then(Value::as_str)
        .unwrap_or("No active station");
    let country = value
        .get("current_country")
        .and_then(Value::as_str)
        .unwrap_or("country unknown");
    let city = value
        .get("current_city")
        .and_then(Value::as_str)
        .unwrap_or("city unknown");
    let mode = value
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("unknown mode");
    let confidence = value
        .get("confidence")
        .and_then(Value::as_str)
        .unwrap_or("unknown confidence");
    let provider_error = value
        .get("provider_error")
        .and_then(Value::as_str)
        .map(|error| format!(" | provider error: {error}"))
        .unwrap_or_default();
    format!("{station} — {city}, {country} | {mode} | {confidence}{provider_error}")
}

fn bundled_plugin_path() -> Option<std::path::PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let name = if cfg!(windows) {
        "roadwaves_scs.dll"
    } else {
        "roadwaves_scs.so"
    };
    let local = executable.parent()?.join("plugins").join(name);
    if local.is_file() {
        return Some(local);
    }
    #[cfg(not(windows))]
    {
        let libdir = executable
            .parent()?
            .parent()?
            .join("lib/roadwaves/plugins")
            .join(name);
        if libdir.is_file() {
            return Some(libdir);
        }
    }
    Some(local)
}

impl Drop for RoadWavesApp {
    fn drop(&mut self) {
        if let Some(mut child) = self.service.take() {
            let _ = child.kill();
        }
    }
}

fn main() -> eframe::Result<()> {
    eframe::run_native(
        "RoadWaves",
        eframe::NativeOptions::default(),
        Box::new(|_| {
            let mut app = RoadWavesApp::default();
            app.start_provider_refresh();
            if app.config.start_with_login {
                app.start_service();
            }
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::summarize_runtime_status;

    #[test]
    fn runtime_status_summary_keeps_station_and_honest_unknowns() {
        let summary = summarize_runtime_status(
            r#"{"current_station":"Local FM","current_country":"DE","current_city":null,"mode":"AutoTune","confidence":"ManualOverride"}"#,
        );
        assert!(summary.contains("Local FM"));
        assert!(summary.contains("city unknown, DE"));
        assert!(summary.contains("ManualOverride"));
    }
}
