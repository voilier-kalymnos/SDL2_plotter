# Rhumb lines and magnetic bearings: design

Agreed on 2026-11-26 in a grilling session. The facts behind the decisions are in [the research note](rhumb-magnetic-research-2026-11-26.md).

## Why

A helmsman and a wind vane hold a constant compass heading. The plotter gives great-circle bearings in degrees true only. A great-circle bearing is correct only at the start of a leg, and a compass shows magnetic, not true. The navigator must convert each bearing by hand.

Also, many fluxgate compasses send an HDG sentence with no variation field. The plotter then shows no heading at all, because it has no variation of its own.

## Terms

- Rhumb line: a line of constant true bearing. On a Mercator chart it is a straight line.
- Variation: the angle between true north and magnetic north at a place and date. East is positive.
- WMM: the World Magnetic Model of NOAA. WMM2025 gives the variation from 2025 to the end of 2029.
- Deviation: the error of one compass in one boat. It is not part of this design.

## Decisions

### Scope and stages

1. One design, delivered in two stages. Stage 1 is the rhumb line functions. They need no outside data. Stage 2 is the magnetic model and the magnetic switch.

### Rhumb lines (stage 1)

2. `gps.rs` gets three functions: the rhumb bearing, the rhumb distance and the point at a fraction of a leg. The formulas and the guard for an east-west leg are in section 3 of the research note.
3. The functions use the same sphere and radius as the great-circle functions (3440.065 NM). The great-circle and rhumb distances then agree on a meridian. There is no ellipsoid.
4. Each function wraps the longitude difference to -180..180 before it calculates, so that a leg across the 180 meridian is short. A latitude of 90 or -90 gives no value.
5. Route legs are rhumb lines, for the bearing, the distance and the drawn line. There is no choice for each route in the first version.
6. The measure tool shows the great-circle distance and bearing as now. If the rhumb values differ by more than 0.5 degrees or 0.1 % of the distance, it also shows them.

### Sign rule and sources (stage 2)

7. The code stores variation in degrees, east positive. `magnetic = true - variation`. The NMEA E/W letter and the Signal K radians are converted where the data enters the app, and nowhere else.
8. `BoatState` gets a variation field with the time it was received. The NMEA decoder publishes the variation from RMC and HDG and does not keep it inside. The Signal K client subscribes to `navigation.magneticVariation`.
9. A live variation expires after 10 s, the same as the fix.
10. If a live variation is valid, the app uses it. If not, the app uses the WMM. The choice of source is one pure function with an injected `Instant`.
11. There is no manual variation setting.

### The magnetic model (stage 2)

12. The WMM is our own `f64` function `variation(position, date)` of about 100 to 130 lines. The coefficient file `WMM.COF` is built into the binary with `include_str!`. There is no new dependency. The parser accepts LF and CR LF line ends.
13. The tests use all 100 rows of the official NOAA test file `WMM2025_TestValues.txt`. The example values in the README of the NOAA zip file are for WMM2020 and are not used.
14. If a valid fix exists, the date for the model is the GPS date. If not, it is the system clock.
15. After the end of 2029 the app keeps using WMM2025. It logs one WARN at start, and the source label says "WMM expired". A date before 2025 means that the clock is wrong. The model then uses the start of 2025, and the app logs one WARN.
16. Near the magnetic poles, where the horizontal field is below 2000 nT, the model gives no value. The bottom bar shows "Var --", and bearings stay true and marked T. The caution band of 2000 to 6000 nT gets no special treatment.
17. The README and the top of the module carry a one-line notice, as 17 U.S.C. 403 requires. It says that the WMM coefficients are a work of the US Government (NOAA NCEI).

### The heading from HDG (stage 2)

18. The true heading from an HDG sentence is magnetic + deviation + variation. The variation comes from the rule of decision 10. The correction moves from the decoder to the session, where the source rule is. An HDG sentence with no variation field then gives a heading, corrected by the WMM.

### What the navigator sees (stage 2)

19. "Magnetic bearings" is one toggle in the menu, with one `AppAction`. It is saved in `state.json`, and it is off by default.
20. With the toggle on, each own-boat bearing shows magnetic with an M mark. These bearings are:
    - the measure tool
    - the bearing to the active waypoint
    - the COG and heading in the bottom bar
    - the bearing from the boat to an AIS target

    With the toggle off, each shows true with a T mark.
    The route has no label for the bearing of each leg, so there is none to mark. The bottom bar read-out is `COG 057°T  HDG 095°T`. The COG shows only with a valid fix and above 1 m/s, the same rule as the course line on the boat marker. The heading also shows with no fix, because a compass works without a GPS. A missing value shows as `--`.
21. The COG and heading that an AIS target sends stay true, because they describe another vessel. They carry a T mark.
22. With the toggle on, the bottom bar shows the variation in use and its source, for example "Var 2°06'W (GPS)" or "Var 1°54'W (WMM)". "GPS" is the label for every live variation, also one from a compass HDG sentence or from Signal K. With the toggle off, it shows no variation, and the layout is as today.
23. The MJPEG stream shows exactly what the bottom bar shows, with no special case.

## Tests

The tests sit at four seams:

- The rhumb line functions in `gps.rs`. Section 3 of the research note gives the expected values. Examples are 60.0405 NM for 1 degree on a meridian and 30.0202 NM at 090 for 1 degree along 60 N. A leg across the antimeridian on the equator is 60.0405 NM east.
- `variation(position, date)` against the 100 NOAA rows, within 0.01 degrees.
- The source choice (live, WMM, WMM expired, no value) with an injected `Instant`.
- The toggle through `Session`, as for the other actions.

## Not in this design

- The deviation card and the compass course of a leg.
- A new model after WMM2025. See "Future WMM updates" in `docs/todo.md`.
- The Signal K COG path `navigation.courseOverGroundLastMinute`, which is not in the Signal K schema. See `docs/todo.md`.
