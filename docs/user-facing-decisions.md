# Key design decisions for the user guide

This list keeps the design decisions that change what the navigator sees or must know. It is the base for a future user guide that explains how the plotter works. Each entry gives the behavior and the reason, and points to the design document that holds the detail.

The test for an entry: without it, a navigator can misread the screen, or can trust a value too much. Internal choices, such as a file format or a thread, do not belong here.

## Position and boat

- With no valid fix for 10 s, the boat marker turns grey at the last position. The plotter does not guess a position. (`CLAUDE.md`, `boat.rs`)
- A new fix that implies more than 50 kn since the last one is rejected, unless three fixes in a row agree. This removes GPS jumps. (`CLAUDE.md`, `FixGuard`)

## Charts

- Where the chart has no tile at the zoom shown, the plotter fills the gap from a coarser level or from another file. A hatch covers the fill. A hatched area is not the chart at this level. (`docs/chart-catalogue-design.md`)
- A photo layer is not a chart. It has no soundings, and the bottom bar names the layer. (`docs/photo-button-design.md`, `docs/three-kinds-design.md`)

## Bearings and magnetic variation

From `docs/magnetic-and-rhumb-design.md`:

- Route legs are rhumb lines: the bearing is constant along the leg, and it is the course to steer, before variation, deviation, leeway and current.
- The measure tool shows the great-circle distance and bearing. If the rhumb values differ, it adds them.
- The bottom bar shows the COG and the heading. The COG shows only with a fix and above about 2 kn, because below that the GPS course is noise. A heading from a compass shows also with no fix.
- Each bearing carries a T (true) or M (magnetic) mark. The "Magnetic bearings" toggle switches all own-boat bearings at once.
- The COG and heading that an AIS target sends are always true.
- If the GPS or the Signal K server sends a variation, the plotter uses it. If not, it uses the World Magnetic Model. The bottom bar names the source. "GPS" means any live source, also a compass or the Signal K server.
- After 2029 the built-in model is out of date. The plotter keeps using it and marks the source "WMM expired".
- Near the magnetic poles the model is not reliable. The plotter then shows "Var --" and true bearings only.
- A compass that sends a magnetic heading with no variation gets its true heading from the model variation. The source label shows this.
- Deviation is not applied. A magnetic bearing is not a compass bearing.
- Distances use a sphere, not the ellipsoid. The error is up to about 0.5 %.
