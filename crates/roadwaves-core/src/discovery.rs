use crate::{
    cache,
    selection::{Candidate, GeoPoint},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashSet, io::Read, net::IpAddr, time::Duration};
use thiserror::Error;
use url::Url;

const MAX_JSON_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Station {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub city: Option<String>,
    pub location: GeoPoint,
    pub country: Option<String>,
    pub stream_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub stream_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StreamInfo {
    pub final_url: String,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayableCandidate {
    pub candidate: Candidate,
    pub stream: StreamInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CachedPlaces {
    pub places: Vec<Station>,
    pub from_cache: bool,
    pub stale: bool,
}
impl Station {
    pub fn candidate(&self) -> Candidate {
        Candidate {
            id: self.id.clone(),
            name: self.name.clone(),
            city: self.city.clone(),
            location: self.location,
            country: self.country.clone(),
            reliable: 0.5,
            playable: self.stream_url.is_some(),
            favorite: false,
            hidden: false,
            successes: 0,
            failures: 0,
        }
    }
}

#[derive(Debug, Error)]
pub enum StationProviderError {
    #[error("provider request failed: {0}")]
    Http(String),
    #[error("provider returned an unsupported response")]
    Schema,
    #[error("provider response exceeded the safety limit")]
    ResponseTooLarge,
    #[error("provider denied access ({0})")]
    Denied(u16),
    #[error("provider stream is not a supported MP3 stream")]
    UnsupportedCodec,
    #[error("provider returned an unsafe stream URL")]
    UnsafeUrl,
}

#[derive(Debug, Deserialize)]
struct Places {
    data: PlaceData,
}
#[derive(Debug, Deserialize)]
struct PlaceData {
    list: Vec<Place>,
}
#[derive(Debug, Deserialize)]
struct Place {
    id: String,
    title: String,
    country: Option<String>,
    geo: [f64; 2],
}

pub struct RadioGardenClient {
    client: reqwest::blocking::Client,
    pub base_url: String,
}
impl Default for RadioGardenClient {
    fn default() -> Self {
        Self::new("https://radio.garden/api")
    }
}
impl RadioGardenClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            client: reqwest::blocking::Client::builder()
                .user_agent("RoadWaves/0.1 (open-source ETS2 companion)")
                .default_headers({
                    let mut headers = reqwest::header::HeaderMap::new();
                    headers.insert(
                        reqwest::header::ACCEPT,
                        "application/json".parse().expect("valid header"),
                    );
                    headers
                })
                .timeout(Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::custom(|attempt| {
                    if attempt.previous().len() >= 4 {
                        attempt.stop()
                    } else if safe_stream_url(attempt.url()) {
                        attempt.follow()
                    } else {
                        attempt.error("unsafe redirect destination")
                    }
                }))
                .build()
                .expect("valid client"),
            base_url: base_url.into().trim_end_matches('/').into(),
        }
    }
    pub fn places(&self) -> Result<Vec<Station>, StationProviderError> {
        let response = self
            .client
            .get(format!("{}/ara/content/places", self.base_url))
            .send()
            .map_err(|e| StationProviderError::Http(e.to_string()))?;
        if response.status() == 401 || response.status() == 403 || response.status() == 429 {
            return Err(StationProviderError::Denied(response.status().as_u16()));
        }
        if !response.status().is_success() {
            return Err(StationProviderError::Http(response.status().to_string()));
        }
        let value = response_json(response)?;
        let body: Places =
            serde_json::from_value(value).map_err(|_| StationProviderError::Schema)?;
        Ok(body
            .data
            .list
            .into_iter()
            .filter_map(|p| {
                let location = GeoPoint {
                    longitude: p.geo[0],
                    latitude: p.geo[1],
                };
                location.valid().then_some(Station {
                    id: p.id,
                    city: Some(p.title.clone()),
                    name: p.title,
                    location,
                    country: p.country,
                    stream_url: None,
                })
            })
            .collect())
    }

    pub fn places_with_cache(
        &self,
        path: &std::path::Path,
    ) -> Result<CachedPlaces, StationProviderError> {
        match self.places() {
            Ok(places) => {
                let _ = cache::store(path, &places, 24 * 60 * 60, cache::now_unix_secs());
                Ok(CachedPlaces {
                    places,
                    from_cache: false,
                    stale: false,
                })
            }
            Err(error) => match cache::load(path, cache::now_unix_secs()) {
                Ok(cached) => Ok(CachedPlaces {
                    places: cached.value,
                    from_cache: true,
                    stale: cached.stale,
                }),
                Err(_) => Err(error),
            },
        }
    }

    pub fn channels_for_place(&self, place_id: &str) -> Result<Vec<Channel>, StationProviderError> {
        let response = self
            .client
            .get(format!(
                "{}/ara/content/page/{}/channels",
                self.base_url,
                escape_path(place_id)
            ))
            .send()
            .map_err(|e| StationProviderError::Http(e.to_string()))?;
        let value = response_json(response)?;
        let mut channels = Vec::new();
        let mut seen = HashSet::new();
        collect_channels(&value, &mut seen, &mut channels);
        Ok(channels)
    }

    pub fn probe_channel_stream(
        &self,
        channel_id: &str,
    ) -> Result<StreamInfo, StationProviderError> {
        let mut response = self
            .client
            .get(format!(
                "{}/ara/content/listen/{}/channel.mp3",
                self.base_url,
                escape_path(channel_id)
            ))
            .header(reqwest::header::RANGE, "bytes=0-3")
            .send()
            .map_err(|e| StationProviderError::Http(e.to_string()))?;
        if response.status() == 401 || response.status() == 403 || response.status() == 429 {
            return Err(StationProviderError::Denied(response.status().as_u16()));
        }
        if !response.status().is_success() {
            return Err(StationProviderError::Http(response.status().to_string()));
        }
        if !safe_stream_url(response.url()) {
            return Err(StationProviderError::UnsafeUrl);
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let mut bytes = [0u8; 4];
        let count = response
            .read(&mut bytes)
            .map_err(|e| StationProviderError::Http(e.to_string()))?;
        let signature_mp3 = count >= 2
            && ((bytes[0] == 0xff && matches!(bytes[1] & 0xe0, 0xe0))
                || &bytes[..count.min(3)] == b"ID3");
        if !signature_mp3 {
            return Err(StationProviderError::UnsupportedCodec);
        }
        Ok(StreamInfo {
            final_url: response.url().to_string(),
            content_type,
        })
    }

    pub fn discover_nearby_playable(
        &self,
        origin: GeoPoint,
        cache_path: &std::path::Path,
    ) -> Result<Vec<PlayableCandidate>, StationProviderError> {
        if !origin.valid() {
            return Err(StationProviderError::Schema);
        }
        let places = self.places_with_cache(cache_path)?.places;
        let mut nearby: Vec<_> = places
            .iter()
            .filter(|place| place.location.valid())
            .filter_map(|place| {
                let distance = origin.distance_km(place.location);
                (distance <= 200.0).then_some((distance, (*place).clone()))
            })
            .collect();
        if nearby.is_empty() {
            nearby = places
                .into_iter()
                .filter(|place| place.location.valid())
                .filter_map(|place| {
                    let distance = origin.distance_km(place.location);
                    (distance <= 400.0).then_some((distance, place))
                })
                .collect();
        }
        nearby.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.id.cmp(&b.1.id)));

        let mut playable = Vec::new();
        for (_, place) in nearby.into_iter().take(5) {
            let channels = self.channels_for_place(&place.id)?;
            for channel in channels.into_iter().take(4) {
                let stream = match self.probe_channel_stream(&channel.id) {
                    Ok(stream) => stream,
                    Err(_) => continue,
                };
                playable.push(PlayableCandidate {
                    candidate: Candidate {
                        id: channel.id,
                        name: channel.name,
                        city: place.city.clone(),
                        location: place.location,
                        country: place.country.clone(),
                        reliable: 0.6,
                        playable: true,
                        favorite: false,
                        hidden: false,
                        successes: 0,
                        failures: 0,
                    },
                    stream,
                });
                if playable.len() >= 12 {
                    return Ok(playable);
                }
            }
        }
        Ok(playable)
    }
}

fn response_json(mut response: reqwest::blocking::Response) -> Result<Value, StationProviderError> {
    if response.status() == 401 || response.status() == 403 || response.status() == 429 {
        return Err(StationProviderError::Denied(response.status().as_u16()));
    }
    if !response.status().is_success() {
        return Err(StationProviderError::Http(response.status().to_string()));
    }
    let mut prefix = [0u8; 1];
    if response.read_exact(&mut prefix).is_err() || prefix[0] == b'<' {
        return Err(StationProviderError::Schema);
    }
    let mut body = vec![prefix[0]];
    let reported_length = response.content_length().unwrap_or(0);
    if reported_length > MAX_JSON_RESPONSE_BYTES as u64 {
        return Err(StationProviderError::ResponseTooLarge);
    }
    response
        .take((MAX_JSON_RESPONSE_BYTES - body.len() + 1) as u64)
        .read_to_end(&mut body)
        .map_err(|e| StationProviderError::Http(e.to_string()))?;
    if body.len() > MAX_JSON_RESPONSE_BYTES {
        return Err(StationProviderError::ResponseTooLarge);
    }
    serde_json::from_slice(&body).map_err(|_| StationProviderError::Schema)
}

fn collect_channels(value: &Value, seen: &mut HashSet<String>, output: &mut Vec<Channel>) {
    match value {
        Value::Object(object) => {
            if let Some(page) = object.get("page").and_then(Value::as_object) {
                let page_url = page.get("url").and_then(Value::as_str);
                let page_name = page.get("title").and_then(Value::as_str);
                if let (Some(page_url), Some(page_name)) = (page_url, page_name) {
                    if let Some(id) = page_url
                        .strip_prefix("/listen/")
                        .and_then(|path| path.rsplit('/').next())
                        .filter(|id| !id.is_empty())
                    {
                        if seen.insert(id.to_owned()) {
                            output.push(Channel {
                                id: id.to_owned(),
                                name: page_name.to_owned(),
                                stream_url: None,
                            });
                        }
                    }
                }
            }
            let id = object.get("id").and_then(Value::as_str);
            let name = object
                .get("title")
                .or_else(|| object.get("name"))
                .and_then(Value::as_str);
            if let (Some(id), Some(name)) = (id, name) {
                if seen.insert(id.to_owned()) {
                    let stream_url = ["streamUrl", "stream_url", "url"].iter().find_map(|key| {
                        object.get(*key).and_then(Value::as_str).map(str::to_owned)
                    });
                    output.push(Channel {
                        id: id.to_owned(),
                        name: name.to_owned(),
                        stream_url,
                    });
                }
            }
            for (key, child) in object {
                if key != "page" {
                    collect_channels(child, seen, output);
                }
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_channels(child, seen, output);
            }
        }
        _ => {}
    }
}

fn escape_path(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('/', "%2F")
        .replace('?', "%3F")
        .replace('#', "%23")
}

fn safe_stream_url(url: &Url) -> bool {
    if !matches!(url.scheme(), "http" | "https") || url.username() != "" || url.host_str().is_none()
    {
        return false;
    }
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1") || host.ends_with(".local") {
        return false;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(ip) => {
                !(ip.is_private()
                    || ip.is_loopback()
                    || ip.is_link_local()
                    || ip.is_unspecified()
                    || ip.is_broadcast())
            }
            IpAddr::V6(ip) => {
                !(ip.is_loopback()
                    || ip.is_unique_local()
                    || ip.is_unicast_link_local()
                    || ip.is_unspecified())
            }
        };
    }
    true
}

pub fn is_safe_stream_url(value: &str) -> bool {
    Url::parse(value)
        .map(|url| safe_stream_url(&url))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn garden_geo_is_lon_lat() {
        let json = r#"{"data":{"list":[{"id":"x","title":"X","country":"DE","geo":[11.5,48.1]}]}}"#;
        let p: Places = serde_json::from_str(json).unwrap();
        let x = &p.data.list[0];
        assert_eq!(x.geo, [11.5, 48.1]);
    }

    #[test]
    fn nested_channels_are_normalized_and_deduplicated() {
        let value: Value = serde_json::from_str(r#"{"data":{"content":[{"items":[{"id":"a","title":"A"},{"id":"a","title":"Duplicate"},{"id":"b","name":"B","url":"https://example.test/b"}]}]}}"#).unwrap();
        let mut seen = HashSet::new();
        let mut channels = Vec::new();
        collect_channels(&value, &mut seen, &mut channels);
        assert_eq!(channels.len(), 2);
        assert_eq!(
            channels[1].stream_url.as_deref(),
            Some("https://example.test/b")
        );
    }

    #[test]
    fn live_page_url_channel_shape_is_supported() {
        let value: Value = serde_json::from_str(r#"{"data":{"content":[{"items":[{"page":{"url":"/listen/asb24music/vEBRr_48","title":"Asb24music","stream":"streamcasthd.com"}}]}]}}"#).unwrap();
        let mut seen = HashSet::new();
        let mut channels = Vec::new();
        collect_channels(&value, &mut seen, &mut channels);
        assert_eq!(
            channels,
            vec![Channel {
                id: "vEBRr_48".into(),
                name: "Asb24music".into(),
                stream_url: None
            }]
        );
    }

    #[test]
    fn unsafe_stream_hosts_are_rejected() {
        assert!(!safe_stream_url(
            &Url::parse("http://127.0.0.1/live.mp3").unwrap()
        ));
        assert!(!safe_stream_url(
            &Url::parse("http://192.168.1.10/live.mp3").unwrap()
        ));
        assert!(!safe_stream_url(&Url::parse("file:///tmp/a.mp3").unwrap()));
        assert!(safe_stream_url(
            &Url::parse("https://radio.example/live.mp3").unwrap()
        ));
    }

    #[test]
    fn cached_places_are_used_when_provider_is_unreachable() {
        let path = std::env::temp_dir().join(format!(
            "roadwaves-places-cache-{}.json",
            std::process::id()
        ));
        let places = vec![Station {
            id: "cached-place".into(),
            name: "Cached Place".into(),
            city: Some("Cached Place".into()),
            location: GeoPoint {
                latitude: 1.0,
                longitude: 2.0,
            },
            country: Some("XX".into()),
            stream_url: None,
        }];
        cache::store(&path, &places, 3600, cache::now_unix_secs()).unwrap();
        let client = RadioGardenClient::new("http://127.0.0.1:1");
        let result = client.places_with_cache(&path).unwrap();
        assert!(result.from_cache);
        assert_eq!(result.places, places);
        let _ = std::fs::remove_file(path);
    }
}
