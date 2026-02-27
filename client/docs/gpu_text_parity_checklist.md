# GPU Text Parity Checklist

Use this checklist before widening rollout beyond `Settings -> Font -> Font Renderer -> GPU`.

## Screenshot Scenarios

- [ ] Tag-only compact territories at zoomed-out view.
- [ ] Tag + time in medium-size territories.
- [ ] Tag + name + time in large territories.
- [ ] Cooldown countdown urgency colors (green/yellow/orange/red).
- [ ] Name truncation and abbreviation behavior.
- [ ] Resource icon placement for single, double, and all-resource territories.
- [ ] Hover and selection interactions near labels/icons.
- [ ] History mode and live mode parity at the same timestamp.

## Performance Gates

- [ ] 240Hz environment: p95 delivered FPS >= 90% of refresh while continuously panning in live mode.
- [ ] 60Hz baseline environment: p95 delivered FPS >= 90% of refresh for the same pan scenario.
- [ ] Pan-only runs show `pan_zero_rebuild_frames > 0` with no static/dynamic/icon rebuild churn.
- [ ] Zoom stress runs show bounded rebuilds tied to zoom-bucket/layout threshold transitions.

## Required Logs

Capture diagnostics from the browser console:

- `gpu-diag static_rebuilds=... dynamic_rebuilds=... icon_rebuilds=... pan_zero_rebuild_frames=...`

Expected trend:

- Pan-only interaction: high `pan_zero_rebuild_frames`, low rebuild counts.
- Time progression: dynamic rebuild count increments at content-boundary cadence.
- Static rebuild count changes primarily on settings/data changes.
