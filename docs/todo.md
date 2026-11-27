# To do

Work that is known but not started. Highest priority first.

## Next round

From the NMEA 0183 work (2026-11-25). See `docs/nmea0183-input-design.md`.

- **Check by eye.** The red and grey boat marker, the track and the AIS
  boat shapes were checked from the log and the unit tests only.
- **No serial transport.** A USB GPS or a serial multiplexer is not
  supported (`tokio-serial` would be needed).
- **NMEA 2000 path not decided.** SocketCAN on the Pi, or a gateway over
  IP (Yacht Devices RAW, Actisense W2K-1). `nmea2k` now logs an ERROR.
  Research: `docs/nmea0183-input-research-2026-11-25.md` section 6.

## AIS, next steps

From `docs/ais-symbology-research-2026-11-25.md` (option C done: colour by
ship type, day-shape marks, class B flat stern).

- **CPA/TCPA alerts: done** (Kalymnos, `collision.rs`, `[collision]`). Still
  open: acknowledge or silence the alarm, a repeated beep while it lasts, and
  no alarm when the own fix is lost during a close approach (now the alarm
  clears).
- **Real-size outlines** from the type 5 dimensions when zoomed in close.
- **AtoN, base station and SART/MOB/EPIRB symbols** (message types 21, 4,
  14; MMSI 970, 972, 974).
- **Signal K AIS class.** The specification schema has no path for it, so
  Signal K targets are drawn as class A.

## Other

- **Future WMM updates.** The magnetic model WMM2025 is valid to the end
  of 2029. After that the app keeps using it and marks the source "WMM
  expired". Decide how a new model (WMM2030) reaches the boat: a new
  release with the coefficients built in, or a coefficient file that the
  navigator can replace.
- **Signal K COG path.** The default COG path
  `navigation.courseOverGroundLastMinute` (`config.rs:396`) is not in the
  Signal K 1.8.4 navigation schema. Find which path the servers send.
  See `docs/rhumb-magnetic-research-2026-11-26.md`.

- **Other config errors still stop the start.** A toml syntax error, or a
  bad value outside the input tables (window, appearance, rendering,
  streaming), fails `AppConfig::load_or_create`, and `main` exits.

- **A panic in a worker thread is not recovered.** The main loop and the
  input decoders survive a panic now; the tile loader and the JPEG encoder
  threads do not restart after one.
- **AIS from the simulator lacks edge cases.** It does not send a target
  that goes silent, "not available" values or tag blocks
  (`docs/simulator-ais-request.md`). Unit tests cover them.
