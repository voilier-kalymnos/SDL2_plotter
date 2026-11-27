# Rhumb lines and magnetic variation: research

Date: 2026-11-26. Scope: rhumb line geodesy for `src/gps.rs`, a World Magnetic Model (WMM) function `variation(position, date)`, one sign rule, and a "show magnetic" switch.

Acronyms used here:

- WMM: World Magnetic Model, the US/UK model of the main geomagnetic field.
- NOAA: US National Oceanic and Atmospheric Administration. NCEI: its National Centers for Environmental Information. BGS: British Geological Survey.
- COF: the WMM coefficient file format (`WMM.COF`).
- BoZ: Blackout Zone. nT: nanotesla. H: horizontal intensity of the field.
- NMEA 0183: the marine serial data standard. RMC, HDG, HDT: NMEA 0183 sentences.
- COG: course over ground. AIS: Automatic Identification System.

## Summary of answers

1. **WMM2025** is valid from 1 January 2025 to 31 December 2029 (epoch 2025.0 to 2030.0). NCEI released it on 17 December 2024. The file is `WMM.COF`: one header line, 90 coefficient lines (degree and order 12), two terminator lines. It is 4,554 bytes. The model and code are public domain. The zip holds 100 official test rows. NOAA gives no extrapolation rule after 2029. In the Blackout Zone (H below 2000 nT) WMM declination is not accurate.
2. **Rust crates.** `world_magnetic_model` 1.2025.0 (12 Sep 2026) is the only maintained general crate with WMM2025 embedded. It needs `uom`, `time`, `thiserror` and `libm`, and works in `f32`. `wmm` 0.2.3 is WMM2020 only and wraps C. `geomag-wmm` 0.1.0 reads WMM2020 from a file. `kinavis-wmm` and `magnetics` embed WMM2025 but bring their own framework crates.
3. **Recommendation: write our own declination function.** A Python prototype written for this research matches all 100 NOAA rows to 0.005 degree in about 60 lines. A Rust version is about 100 to 130 lines plus `include_str!` of the 4,554 byte file. It adds no dependency and works in `f64` with `chrono`, which the repo already uses.
4. **Rhumb formulas** come from the Aviation Formulary and the Veness reference code. The bearing is `atan2(dlon, dpsi)`. The distance is `sqrt(dlat^2 + q^2 dlon^2) R`. For an east-west leg (`|dpsi|` below 1e-12), `q = cos(lat1)`. Wrap `dlon` into -180..180. The formulas fail at a pole. The repo radius is 3440.065 NM, so 1 degree of arc is 60.0405 NM.
5. **Repo today.** `gps.rs` has only great circle functions. NMEA RMC variation is kept inside the decoder, and HDG becomes true heading as `magnetic + deviation + variation`. `BoatState` stores no variation. Signal K `navigation.magneticVariation` is not read.
6. **Signal K.** `navigation.magneticVariation` is in radians, east positive, and is "added to the magnetic heading to derive the true heading". `navigation.magneticVariationAgeOfService` (seconds since 1970) is the only model metadata.
7. **Sign rule.** WMM declination D is clockwise (east) from true north. Signal K uses the same sign. The rule "magnetic = true minus variation, east positive" agrees with both and with the repo's HDG code.

## 1. WMM2025

### Validity, release, format

| Fact | Value | Source |
|---|---|---|
| Validity | 1 January 2025 to 31 December 2029 | WMM2025 Technical Report, section 3, line "valid from January 1, 2025 to December 31, 2029" [R1] |
| Epoch | 2025.0. "WMM is valid for five years from the Model Epoch" | NCEI WMM page, coefficient format section [N1] |
| Expiry | "the current model expires on December 31, 2029" | [N1] |
| Release | 17 December 2024 (WMM2025 and WMMHR2025 released) | [N1], Updates list. README in the zip is dated "December 17, 2024" [Z1] |
| Header line | `2025.0 WMM-2025 11/13/2024` (11/13/2024 is the coefficient date, US style, no effect on the calculation) | `WMM.COF` line 1 [Z1], [N1] |
| Degree and order | 12. The main field has 168 Gauss coefficients, the secular variation model is also degree and order 12 | [N1], Background |
| File layout | Line 1 header. Lines 2 to 91 coefficients `n m g h g_dot h_dot`. Lines 92 to 93 terminators of 48 `9` digits | [N1], "WMM Format" |
| Read format (C) | `"%3d%3d%10.1f%10.1f%11.1f%11.1f"`. Stop at the terminator line | [N1] |
| Units | g and h in nT at the epoch, g_dot and h_dot in nT per year | [N1], [R1] section 1.2 |
| Format stability | NCEI says the COF format "is permanent" | [N1] |
| File size | `WMM.COF` is 4,554 bytes, 93 lines. The zip `WMM2025COF.zip` is 42,887 bytes | Measured on download 2026-11-26 [Z1] |
| Line ends | The NCEI page says CR LF. The file in the zip has LF line ends (confirmed with `file`). A parser must accept both | [N1], [Z1] |
| SHA-256 of `WMM.COF` | `dfa8597825af4e0b87ff4198a5b4fb661b3c49f4cd090cd0164e0259b075582f` | Computed 2026-11-26 [Z1] |

First and last coefficient lines, from the file [Z1]:

```
  1  0  -29351.8       0.0       12.0        0.0
  1  1   -1410.8    4545.4        9.7      -21.5
 12 12      -0.7       0.2       -0.1       -0.1
```

### Licence

NCEI says the WMM source code is in the public domain. It has no licence and no copyright. The public can use the information and software freely [N1], Citations. Under 17 U.S.C. 403, a copyrighted work made mainly of this material must carry a notice. The notice names the US Government material. A one-line notice beside the embedded file satisfies this.

### Download

- Coefficients and test values: https://www.ncei.noaa.gov/sites/default/files/2024-12/WMM2025COF.zip [N2]. It contains `WMM.COF`, `WMM2025.COF` (same bytes), `WMM2025_TestValues.txt`, `WMM2025_TestValues.pdf` and `README-WMM-COEFS.txt` [Z1].
- Test values PDF also at https://www.ncei.noaa.gov/sites/default/files/2025-02/WMM2025testvalues.pdf [N1].

### Official test values

`WMM2025_TestValues.txt` has 100 rows, 10 at each date 2025.0, 2025.5, ... 2029.5 [Z1]. The columns are:

1. Decimal year.
2. Height above the WGS 84 ellipsoid, in **km**.
3. Geodetic latitude, then geodetic longitude.
4. Declination D in degrees, given to 0.01 degree.
5. I, H, X, Y, Z, F and their annual rates.

Selected rows, copied exactly (lon is east positive, the repo order is `(lon, lat)`):

| Year | Height km | Lat | Lon | D (deg) | H (nT) | Note |
|---|---|---|---|---|---|---|
| 2025.0 | 28 | 89 | -121 | -99.77 | 1504.30 | In the Blackout Zone (H < 2000) |
| 2025.0 | 66 | 14 | 143 | -0.19 | 35003.64 | |
| 2025.0 | 18 | 0 | 21 | 1.29 | 29282.25 | |
| 2025.5 | 6 | -36 | -137 | 20.28 | 25353.23 | |
| 2025.5 | 63 | 26 | 81 | 0.51 | 34803.98 | |
| 2025.5 | 8 | -52 | -75 | 14.91 | 20005.51 | |
| 2027.5 | 0 | -13 | -59 | -17.49 | 22401.49 | The only row at height 0 |

The plotter works at sea level. Most rows have a height above zero, so a test that uses all rows must pass the height to the function. The function can take height in km and the app can pass 0.

Warning: `README-WMM-COEFS.txt` in the 2025 zip still shows example rows dated 2020.0 and 2020.5 (for example `2020.0 66 14 143 0.12`). These are WMM2020 values. Do not copy them into a test [Z1].

### Use outside the validity period

- NCEI says the model "expires on December 31, 2029" [N1]. The online calculator accepts dates from 13 November 2024 to 31 December 2029 only [N1].
- NCEI says WMM2025 supersedes WMM2020 and must replace it in navigation systems [N1], Background (paraphrased).
- Not confirmed: a NOAA statement on the error of an extrapolation past 2029. The report gives error budgets at 2025.0 and 2030.0 only [R1] section 3.
- WMM2030 is due near the end of 2029, by the five-year cycle [N1]. Not confirmed: the exact date.

### Blackout and Caution Zones

- BoZ: regions around the north and south magnetic poles where H is less than 2000 nT. There, WMM declination values are not accurate and compasses are unreliable [N3], [R1] section 1.8.
- Caution Zone: 2000 nT <= H < 6000 nT around each BoZ [N3], [R1] section 1.8.
- The report quotes an NGA warning [R1] section 1.8. Near the BoZ, declination errors exceed 1 degree. Inside the BoZ, they reach 180 degrees.
- The zones move with the poles. WMM2025 gives BoZ locations until December 2029 [R1] section 1.8.
- Location-dependent declination error (1 sigma): `sigma_D = sqrt(0.26^2 + (5417 / H)^2)` degrees, H in nT [R1] section 3.4. The constants 0.26 and 5417 are confirmed in the `world_magnetic_model` source (`wmm_models.rs:53-54`) [C1]. The global average is 0.41 degree [R1].
- Local crustal anomalies are not in the model. NCEI says anomalies above 10 degrees exist, and 3 to 4 degrees "are not uncommon" [N3], Limitations.

Consequence for the plotter: the function must compute H as well as D. When H < 2000 nT it must return "no variation". When H < 6000 nT the UI can mark the value as uncertain.

## 2. Rust crates

All facts below come from the crates.io API and from the published source, downloaded 2026-11-26.

| Crate | Version, date | Licence | WMM2025 | Coefficients | Dependencies | no_std | NOAA test values |
|---|---|---|---|---|---|---|---|
| `world_magnetic_model` [C1] | 1.2025.0, 2026-09-12 | MIT OR Apache-2.0 | Yes, plus WMM2020 | Embedded as `f32` constants (`src/wmm_data.rs`) | `uom` 0.38 (si, f32), `time` 0.3, `thiserror` 2, `libm` 0.2, `serde` optional | Yes (`#![no_std]`, `lib.rs:1`) | In the git repo yes. In the published crate the test file is excluded (`exclude = [... "ncei.noaa.gov/"]`), so `src/tests/official.rs` is ignored there |
| `wmm` [C2] | 0.2.3, 2022-11-14 | MIT OR Apache-2.0 | No. Dates 2020 to 2024 only (`src/lib.rs`, `!(2020..=2024).contains(&year)`) | Embedded in C (`lib/WMM_COF.c`), built with `cc` | `cc`, `cty`, `spin`, `time` | Yes | Not confirmed |
| `geomag-wmm` [C3] | 0.1.0, 2024-09-28 | MIT | No. Ships `WMM.2020.COF` | Read from a file path at run time (`initialise_magnetic_model(path)`) | none | No | Not confirmed |
| `kinavis-wmm` [C4] | 0.1.0, 2026-09-23 | MIT OR Apache-2.0 | Yes (feature `wmm2025`) | Embedded | `kinavis-kernel` 1.x (own value types) | Yes, with `libm` feature | Yes, all 100 rows (`tests/noaa_test_values.rs`, tolerance 0.005 deg) |
| `magnetics` [C5] | 0.1.9, 2026-10-27 | MIT | Yes | `include_str!("../data/WMM2025.COF")` | `geopotential`, `metrology` | Not confirmed | Yes, all 100 rows (`src/main_field.rs:198-203`) |

Details for `world_magnetic_model` [C1]:

- API: `GeomagneticField::new(height: Length, latitude: Angle, longitude: Angle, date: time::Date) -> Result<_, Error>`. Then `declination()`, `declination_uncertainty()`, `declination_warning() -> Option<WarningZone>` with `BlackoutZone` and `CautionZone`.
- Outside 1 January 2020 to 31 December 2029 it returns `Error::DateOutsideOfValidityRange` (`src/error.rs:9`, `src/wmm_models.rs:72-100`). It does not extrapolate.
- The date resolution is one day (`years_since_epoch`, `lib.rs:707-709`).
- Downloads: 233,894 at the time of reading. It is the most used crate.
- Cost for this repo: two new runtime crates (`uom`, `time`) next to `chrono`, and a `chrono` to `time::Date` conversion. `uom` uses heavy generics. Not confirmed: its build time on a Raspberry Pi.

Other search results (`igrf`, `spacelab-geomagnetism`, `tobari`, `satkit`) are not WMM or are much larger science libraries. They were not read in detail.

### Own implementation: size estimate

A Python prototype was written for this research (scratchpad only, not in the repo). It does these steps:

1. Read `WMM.COF`.
2. Convert the geodetic position to geocentric.
3. Run the Schmidt semi-normalised Legendre recursion to degree 12.
4. Sum X, Y and Z, then rotate X to the ellipsoid frame.
5. Return `atan2(Y, X)`.

Result over all 100 official rows: worst declination error 0.005 degree, which is the rounding of the published 0.01 degree values. The core is about 60 lines.

Estimate for Rust in `f64`:

| Part | Lines |
|---|---|
| COF parser (header epoch, 90 rows, stop at `9999`) | 20 to 25 |
| Geodetic to geocentric, Legendre recursion, sum, rotation | 60 to 75 |
| H, Blackout Zone test, validity window, decimal year from `chrono` | 15 to 25 |
| Total code | 100 to 130 |
| Tests (all 100 rows through `include_str!`, BoZ row, date window) | 40 to 60 |

The coefficients go in with `include_str!("wmm/WMM.COF")` (4,554 bytes). A new model in 2029 is a file swap, as NCEI intends [N1].

### Recommendation

Write our own function. Reasons:

- It adds no dependency. The repo uses `chrono`, not `time` (`Cargo.toml`, `[dependencies]`).
- It is `f64`, like the rest of `gps.rs`.
- It is small (about 120 lines) and the 100 NOAA rows test it completely.
- The policy outside 2025 to 2029 and inside the BoZ is ours to choose. It can return `None` and the plotter degrades to "true only", which follows the "degrade, never exit" rule.
- The model is public domain. The file needs only the 17 U.S.C. 403 notice.

If the team prefers a crate, use `world_magnetic_model` 1.2025.0. Its behaviour outside the window (an error) and its warning zones match the needs. It costs `uom` and `time` and gives `f32` results.

Design note, not confirmed by a source: the date for the model must come from a trusted clock. The RMC sentence carries the UTC date (field 9, `ddmmyy`) [G1]. A Pi without network time can start with a wrong clock. A date outside the window then gives `None`, which is safe.

## 3. Rhumb line formulas

A rhumb line (loxodrome) is a track of constant true course. It is a straight line on a Mercator projection [W1], [V1]. The chart renderer uses Web Mercator (`mbtiles_renderer/projection.rs`), so a straight route leg on the screen is a rhumb line.

Notation: latitude phi, longitude lambda (east positive), all in radians. `psi(phi) = ln(tan(pi/4 + phi/2))` is the isometric latitude, equal to `atanh(sin phi)` [V1]. R is the earth radius.

### Bearing

```
dlambda = lambda2 - lambda1, wrapped into -pi..pi
dpsi    = psi(phi2) - psi(phi1)
theta   = atan2(dlambda, dpsi), then mod 360 degrees
```

Sources: Veness `rhumbBearingTo`, `latlon-spherical.js` lines 638 to 657 [V2]. Aviation Formulary "Rhumb Line Navigation" gives the same with west-positive longitude: `tc = mod(atan2(lon1-lon2, dphi), 2 pi)` [W1].

### Distance

```
dphi = phi2 - phi1
q    = dphi / dpsi     when |dpsi| > 1e-12
q    = cos(phi1)       otherwise (east-west leg)
d    = sqrt(dphi^2 + q^2 * dlambda^2) * R
```

Sources: Veness `rhumbDistanceTo`, lines 601 to 627, guard `Math.abs(Δψ) > 10e-12` at line 617 [V2]. The Formulary uses the guard `abs(lat2-lat1) < sqrt(TOL)` with TOL near 1e-15, "to avoid 0/0 indeterminacies on E-W courses" [W1]. Both guards give the same result in practice. The guard on `dpsi` is direct: it tests the divisor.

### Point at fraction f of the leg

Distance along a rhumb line is proportional to the change in latitude (`d = dphi / cos(theta)`), so fraction f of the distance is at `phi = phi1 + f * dphi`. The track is a straight line in (lambda, psi), so:

```
phi    = phi1 + f * dphi
lambda = lambda1 + dlambda * (psi(phi) - psi(phi1)) / dpsi    when |dpsi| > 1e-12
lambda = lambda1 + f * dlambda                                  otherwise
then wrap lambda into -pi..pi
```

The Formulary gives the point at course tc and distance d [W1]. Veness gives it as `rhumbDestinationPoint`, lines 671 to 696 [V2]. Use `d = f * total`. The closed form above was compared with the destination-point form for two legs below. The results agree to 1e-12 degree.

### Antimeridian

Both sources take the shorter rhumb line. Veness: "if dLon over 180° take shorter rhumb line across the anti-meridian" [V2] line 612. The Formulary computes `dlon_W` and `dlon_E` and takes the smaller [W1]. For an exact 180 degree difference, the direction is a choice. For equal values, the Formulary picks east. Its test is `if (dlon_W < dlon_E)` for west, else east [W1]. The result longitude must be wrapped back into -180..180 (`lon=mod(lon1+dlon+pi,2*pi)-pi`) [W1]. At exactly 180 the wrap can give -180 or +180, so a test must compare longitudes modulo 360.

### Poles

- The Formulary says that near a pole, rhumb lines "become tightly wound spirals". It also says the formulas fail at a pole [W1].
- `psi(phi)` is infinite at +-90 degrees [V1].
- Veness folds a destination latitude past the pole back (line 679), and the Formulary reports "d too large" [W1], [V2].
- The repo accepts latitude -90..=90 at the boundary (CLAUDE.md). Web Mercator charts stop near 85.05 degrees, so a user click cannot reach a pole, but a GPX file can. If either latitude is +-90, the rhumb functions must return `None`.
- Coincident points: Veness returns `NaN` for the bearing (line 640). The repo function needs a defined value, for example bearing 0 and distance 0.

### Test values with the repo radius

`gps.rs:8` defines `EARTH_RADIUS_NM = 3440.065`. One degree of arc is `3440.065 * pi / 180 = 60.0405 NM`. The values below use that radius. They were computed with an independent Python script (scratchpad), and the arithmetic is shown.

| Leg `(lon, lat)` | Arithmetic | Rhumb distance | Rhumb bearing | Midpoint (f = 0.5) | Great circle, for comparison |
|---|---|---|---|---|---|
| (0, 0) to (0, 1) | meridian, `d = dphi * R = 1 deg * 60.0405` | 60.0405 NM | 0.000 | (0, 0.5) | same |
| (0, 60) to (1, 60) | `dpsi = 0`, `q = cos 60 = 0.5`, `d = 0.5 * 60.0405` | 30.0202 NM | 90.000 | (0.5, 60) | 30.0199 NM at 89.567 initial |
| (179.5, 0) to (-179.5, 0) | `dlambda = -359 deg` wraps to +1 deg, `q = cos 0 = 1` | 60.0405 NM | 90.000 (east) | (180 or -180, 0) | same |
| (0, 0) to (1, 1) | `psi(1 deg) = atanh(sin 1 deg) = 0.0174542`, `dlambda = 0.0174533`, `theta = atan2(0.0174533, 0.0174542) = 44.9985 deg`, `d = dphi / cos theta * R` | 84.9079 NM | 44.9985 | (0.49998, 0.5) | 84.9079 NM at 44.9956 |
| (-4.5, 48.4) to (-1.6, 50.1) | `dpsi = 0.0454602`, `dlambda = 2.9 deg`, `q = 0.652671` | 152.7495 NM | 48.0709 | (-3.06248, 49.25) | 152.7401 NM at 46.9832 |
| (0, 89) to (180, 89) | `dpsi = 0`, `q = cos 89 = 0.0174524` | 188.6131 NM | 90.000 | | 120.0809 NM over the pole |

Compare with the Formulary's LAX to JFK example: it gives 0.629650 rad and 79.32 degrees [W1]. With R = 3440.065 the distance is `0.629650 * 3440.065 = 2166.04 NM`. The script gives 2166.037 NM and 79.324 degrees. The Formulary prints 2164.6 NM because it uses 1 NM = 1 minute of arc (R = 3437.75 NM).

The repo test style already uses `ONE_DEGREE_NM = 60.04` (`gps.rs:62`). The first three rows reuse that constant.

## 4. The current repo

### `src/gps.rs` (129 lines)

| Item | Location |
|---|---|
| Module doc: great circle on a sphere, `(lon, lat)` in degrees, bearings 0..360 clockwise from true north | `src/gps.rs:1-5` |
| `pub const EARTH_RADIUS_NM: f64 = 3440.065` | `src/gps.rs:8` |
| `pub const EARTH_RADIUS_METERS: f64 = 6_371_000.0` | `src/gps.rs:10` |
| `fn central_angle(a, b) -> f64` (haversine, clamps `h` to 1) | `src/gps.rs:13-25` |
| `pub fn distance_nm(a: (f64, f64), b: (f64, f64)) -> f64` | `src/gps.rs:28` |
| `pub fn distance_meters(a, b) -> f64` | `src/gps.rs:33` |
| `pub fn bearing_deg(a, b) -> f64` (initial great circle bearing) | `src/gps.rs:38-48` |
| `pub fn distance_bearing_nm(a, b) -> (f64, f64)` | `src/gps.rs:51` |
| Tests: `one_degree_of_latitude_is_sixty_nautical_miles` (66), `one_degree_of_latitude_in_metres` (72), `points_are_lon_lat_so_a_latitude_step_at_any_longitude_is_one_degree` (78), `distance_is_zero_for_the_same_point_and_symmetric` (86), `bearing_follows_the_compass` (94), `distance_and_bearing_agree_with_the_separate_functions` (115), `antipodal_points_do_not_produce_nan` (124) | `src/gps.rs:66-127` |

There is no rhumb line function and no magnetic function.

### Where variation is read today

| Fact | Location |
|---|---|
| The NMEA decoder keeps `variation: Option<f64>`, the last RMC variation, in degrees, east positive. An HDG without its own variation uses it. | `src/nmea0183.rs:79-81` |
| RMC: fields 10 and 11 (value and E/W) set `self.variation` | `src/nmea0183.rs:182-186` |
| HDG: variation from fields 4 and 5, else the RMC variation. Deviation from fields 2 and 3. If they are empty, deviation is 0. True heading = `magnetic + deviation + variation` | `src/nmea0183.rs:164-177` |
| HDG is ignored while HDT is fresh | `src/nmea0183.rs:163`, `355` |
| `offset()`: E positive, W negative, rejected above 180 degrees | `src/nmea0183.rs:434-437` |
| `heading()` stores `degrees_true.rem_euclid(360).to_radians()` in `BoatState::heading` | `src/nmea0183.rs:362-366` |
| HDG with no variation at all gives no heading | test `hdg_without_any_variation_gives_no_heading`, `src/nmea0183.rs:764-770` |
| Tests of the sign: `HDG,100,2.0,E,5.0,W` gives 97, `2.0,W,5.0,W` gives 93 | `src/nmea0183.rs:720-726` |
| `BoatState` has `heading: Option<f64>` (true, radians) and no variation field | `src/boat.rs:15-28` |
| Signal K: only `position`, `course_over_ground`, `speed_over_ground`, `heading` and AIS paths are configured. No variation path | `src/config.rs:94-106`, defaults `src/config.rs:395-398` |
| Signal K heading decode (radians, finite values only) | `src/signalk.rs:322-327` |

The field order matches the RMC and HDG definitions in the GPSD "NMEA Revealed" document: RMC field 10 "Magnetic Variation, degrees", field 11 "E or W" [G1]. HDG fields: 1 magnetic sensor heading, 2 deviation, 3 E/W, 4 variation, 5 E/W [G1]. The GPSD page does not state the arithmetic sign rule. Not confirmed from the NMEA standard itself (paid document).

Observation: with a WMM, an HDG sentence with no variation and no RMC variation can still give a true heading. Today it gives none (`src/nmea0183.rs:764-770`).

Observation: the default COG path `navigation.courseOverGroundLastMinute` (`src/config.rs:396`) is not in the property list of the Signal K `navigation.json` schema, version 1.8.4 [S1]. The schema has `courseOverGroundTrue` and `courseOverGroundMagnetic`. This is outside the scope of this note.

### Where the UI shows bearings today

All bearings shown today are great circle initial bearings, true, from `gps::distance_bearing_nm`.

| Place | Text | Location |
|---|---|---|
| Measure tool result | `"{:.1} Mn {:.0}°"` or `"{:.0} m {:.0}°"` | `src/mesure.rs:40`, `70`, `86-88` |
| Route: boat to active waypoint | `calc_route_mn_from_boat` returns distance, bearing and total | `src/route.rs:163-169` |
| Bottom bar | `"From Boat to Next Waypoint :  {:.1} Mn   {:.0}° / DTD : {:.1} Mn"` | `src/app/render.rs:236-238` |
| AIS details window: range and bearing | `"{distance:.2} NM, bearing {:03}°"` | `src/ais.rs:136-139` |
| AIS details window: target course and heading | `"{:03}°"` | `src/ais.rs:173`, `179` |

A "show magnetic" switch touches these five places. The AIS course and heading come from the target as true values.

## 5. Signal K

From `schemas/groups/navigation.json` in the SignalK/specification repository, schema id 1.8.4 [S1]:

| Path | Units | Description (quoted) |
|---|---|---|
| `navigation.magneticVariation` | rad | "The magnetic variation (declination) at the current position that must be added to the magnetic heading to derive the true heading. Easterly variations are positive and Westerly variations are negative (in Radians)." |
| `navigation.magneticVariationAgeOfService` | s | "Seconds since the 1st Jan 1970 that the variation calculation was made" |
| `navigation.headingMagnetic` | rad | "Current magnetic heading of the vessel, equals 'headingCompass adjusted for magneticDeviation'" |
| `navigation.magneticDeviation` | rad | "Magnetic deviation of the compass at the current headingCompass" |
| `navigation.headingTrue` | rad | "The current true north heading of the vessel, equals 'headingMagnetic adjusted for magneticVariation'" |
| `navigation.courseOverGroundTrue` / `courseOverGroundMagnetic` | rad | "Course over ground (true)" / "(magnetic)" |

- Sign: east positive, `true = magnetic + variation`. This is the same rule as WMM D [R1] section 1.1 and the repo's HDG code.
- Model or source fields: the schema has no field that names a model (for example "WMM2025"). The only metadata is `magneticVariationAgeOfService`. The general Signal K `$source` of each value tells which device sent it. Not confirmed: a standard way to tell a GPS value from a computed one.
- The schema also has `navigation.courseRhumbline` and `navigation.courseGreatCircle` objects [S1]. They were not read in detail.

### Sign rule for the plotter

- WMM: D is measured clockwise from true north to the horizontal field [R1] section 1.1. The report also calls D the magnetic variation.
- Signal K: east positive, added to magnetic to get true [S1].
- Repo: `offset()` makes E positive, and HDG true = magnetic + deviation + variation (`src/nmea0183.rs:175`, `434-437`).
- Therefore: `magnetic = true - variation` and `true = magnetic + variation`, variation east positive. All three sources agree.

## Sources

NOAA / NCEI:

- [N1] NCEI, World Magnetic Model main page: https://www.ncei.noaa.gov/products/world-magnetic-model (read 2026-11-26).
- [N2] NCEI, WMM Coefficients page: https://www.ncei.noaa.gov/products/world-magnetic-model/wmm-coefficients
- [N3] NCEI, WMM Accuracy, Limitations, and Error Model: https://www.ncei.noaa.gov/products/world-magnetic-model/accuracy-limitations-error-model
- [R1] Chulliat, A. et al., 2025. The US/UK World Magnetic Model for 2025-2030: Technical Report. NCEI. https://doi.org/10.25923/prbc-s316 (PDF: https://library.oarcloud.noaa.gov/noaa_documents.lib/NESDIS/NCEI/WWM_Report_2025.pdf). Sections 1.1, 1.2, 1.8, 3, 3.4.
- [Z1] Contents of https://www.ncei.noaa.gov/sites/default/files/2024-12/WMM2025COF.zip: `WMM.COF`, `WMM2025_TestValues.txt`, `README-WMM-COEFS.txt` (downloaded 2026-11-26).

Rust crates (crates.io API and published source):

- [C1] `world_magnetic_model` 1.2025.0: https://crates.io/crates/world_magnetic_model, repo https://github.com/budzejko/world_magnetic_model, docs https://docs.rs/world_magnetic_model/latest/
- [C2] `wmm` 0.2.3: https://crates.io/crates/wmm, repo https://github.com/sevenseas-io/wmm
- [C3] `geomag-wmm` 0.1.0: https://crates.io/crates/geomag-wmm, repo https://git.hostux.fr/ConstellationVFR/geomag-wmm
- [C4] `kinavis-wmm` 0.1.0: https://crates.io/crates/kinavis-wmm, repo https://github.com/KINAVIS/kinavis
- [C5] `magnetics` 0.1.9: https://crates.io/crates/magnetics, repo https://github.com/FusionVault/physica

Rhumb lines:

- [W1] Ed Williams, Aviation Formulary V1.47, "Rhumb Line Navigation": https://edwilliams.org/avform147.htm
- [V1] Chris Veness, "Calculate distance, bearing and more between Latitude/Longitude points", section "Rhumb lines": https://www.movable-type.co.uk/scripts/latlong.html
- [V2] Chris Veness, `latlon-spherical.js` (MIT), reference implementation: https://github.com/chrisveness/geodesy/blob/master/latlon-spherical.js (lines 601 to 735, read 2026-11-26).

NMEA 0183 and Signal K:

- [G1] Eric S. Raymond, "NMEA Revealed", GPSD project: https://gpsd.gitlab.io/gpsd/NMEA.html, sections RMC and HDG. The NMEA 0183 standard itself is a paid document and was not read.
- [S1] Signal K specification, `schemas/groups/navigation.json`: https://github.com/SignalK/specification/blob/master/schemas/groups/navigation.json

Repo files, at commit 77ed7a2 plus the working tree of 2026-11-26:

- `src/gps.rs`, `src/nmea0183.rs`, `src/boat.rs`, `src/signalk.rs`, `src/config.rs`.
- `src/mesure.rs`, `src/route.rs`, `src/app/render.rs`, `src/ais.rs`, `Cargo.toml`.
