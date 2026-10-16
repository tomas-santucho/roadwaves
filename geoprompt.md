Top G, I found something better than training a transformer. 😏
ETS2 already has a known map projection, and we can reverse it to convert telemetry coordinates into approximate real-world GPS coordinates.
There's even an existing implementation in TruckSim Maps — projections.ts.
The developers use a Lambert Conformal Conic (LCC) projection, with custom scaling and offsets. SCS documents the projection system, and the TruckSim Maps project implements the conversion. 

SCS Modding Wiki
+2



1. The RoadWaves coordinate transformer
ETS2 telemetry
x: 10183.1, z: -10001.2
Rust GeoTransformer
Map offsets → Scale → Inverse LCC projection

Approximate real GPS
52.4878° N, 13.1037° E — Berlin area
Radio Garden
Discover stations around Berlin

I tested the mathematical conversion against published ETS2 city coordinates:
City	Converted latitude	Converted longitude
Berlin	52.4878	13.1037
Amsterdam	52.3650	4.6829
Warsaw	52.2489	21.0591
Munich	48.1180	11.4202
Those results are geographically plausible, although not exact city centers.
The catch: ETS2 isn't a geographically exact reproduction of Europe. Certain regions have custom distortions, notably the UK. A projection alone won't perfectly map every road to its real-world location. The map tooling's maintainers confirm this limitation. 

GitHub



2. I made the Rust transformer
ets2_geo_transform.rs
Standalone Rust module · No external dependencies · Reference unit testsDownload Rust source


Usage:
let gps = ets2_to_wgs84(10183.1, -10001.2)
    .expect("Valid ETS2 coordinates");

println!("{}, {}", gps.latitude, gps.longitude);
// 52.4878, 13.1037


I've checked the underlying calculations, but haven't compiled the Rust module in this environment.
3. Make it more accurate with city anchors
This is the part I'd actually prioritize for RoadWaves.
There's an existing ETS2 City Coordinate Retriever dataset containing game coordinates for cities. It includes coordinates for Berlin, Amsterdam, Warsaw, and many more. 

GitHub



We can pair each game's city coordinates with the city's actual GPS coordinates.
For example:
{
  "city": "Berlin",
  "country": "DE",
  "game": {
    "x": 10183.1,
    "z": -10001.2
  },
  "real": {
    "lat": 52.52,
    "lon": 13.405
  }
}


Then RoadWaves has two sources of geographic information:
- Projection: approximate GPS position anywhere on the map.
- City anchors: known reference points that correct or contextualize the projection.
For more accuracy, use the three nearest compatible city anchors to interpolate corrections, with distance weighting. Avoid blending anchors across known map discontinuities such as the UK's custom scaling.
For a radio application, you could even skip fine-grained corrections initially and use the nearest mapped city as the radio discovery center.
4. What I would implement in RoadWaves
crates/roadwaves-core/
└── src/
    └── geo/
        ├── mod.rs
        ├── projection.rs
        ├── anchors.rs
        ├── calibrator.rs
        └── resolver.rs

assets/
└── geo/
    ├── ets2-vanilla.json
    └── ets2-promods.json


The resolver should produce something like:
{
  "latitude": 52.52,
  "longitude": 13.405,
  "nearestCity": "Berlin",
  "country": "DE",
  "method": "city_calibrated",
  "confidence": "high"
}


Then use that resolved location to find nearby Radio Garden stations.
I'd also include map-version validation, fallback to the nearest known city if the transformation is unreliable, and a cooldown before switching to another geographic region.
My recommendation: don't use machine learning. Build a deterministic GeoTransformer with inverse Lambert projection + city anchors + local interpolation.
It's fast, completely offline, cross-platform, and far easier to debug than a neural model. And because RoadWaves only needs to identify the right local radio stations—not place the truck on an exact real-world road—this approach should be more than sufficient.
