/// Tempo description for the project. Either a single constant BPM or a
/// piecewise-linear automation curve over beats.
#[derive(Debug, Clone, PartialEq)]
pub enum Tempo {
    Constant(f64),
    Automated(AutomatedTempo),
}

/// Piecewise-linear tempo automation. Points are sorted by beat ascending and
/// always include a synthetic point at beat 0 (set to the initial BPM). Past
/// the final point, the last point's BPM is held (extrapolation).
#[derive(Debug, Clone, PartialEq)]
pub struct AutomatedTempo {
    points: Vec<TempoPoint>,
    cumulative_seconds: Vec<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempoPoint {
    pub beat: f64,
    pub bpm: f64,
}

impl Tempo {
    /// Build an `Automated` tempo from raw envelope events. Events with
    /// negative beats are treated as the initial value (the latest such event
    /// wins). If no negative-beat event exists, `fallback_initial_bpm` is used.
    pub fn from_automation_events(
        mut events: Vec<TempoPoint>,
        fallback_initial_bpm: f64,
    ) -> Self {
        // Initial value at beat 0: latest negative-beat event, else fallback.
        let initial_bpm = events
            .iter()
            .filter(|p| p.beat < 0.0)
            .max_by(|a, b| a.beat.partial_cmp(&b.beat).unwrap_or(std::cmp::Ordering::Equal))
            .map(|p| p.bpm)
            .unwrap_or(fallback_initial_bpm);

        events.retain(|p| p.beat >= 0.0);
        events.sort_by(|a, b| a.beat.partial_cmp(&b.beat).unwrap_or(std::cmp::Ordering::Equal));

        // Ensure beat 0 anchor exists.
        if events.first().map(|p| p.beat).unwrap_or(f64::INFINITY) > 0.0 {
            events.insert(
                0,
                TempoPoint {
                    beat: 0.0,
                    bpm: initial_bpm,
                },
            );
        }

        // Deduplicate identical-beat points (keep the last one — Ableton can
        // emit several with identical times when an envelope is edited).
        let mut deduped: Vec<TempoPoint> = Vec::with_capacity(events.len());
        for ev in events {
            match deduped.last_mut() {
                Some(last) if (last.beat - ev.beat).abs() < f64::EPSILON => *last = ev,
                _ => deduped.push(ev),
            }
        }
        let events = deduped;

        // If all points collapsed to a single one, prefer constant tempo.
        if events.len() == 1 {
            return Tempo::Constant(events[0].bpm);
        }

        let mut cumulative_seconds = Vec::with_capacity(events.len());
        cumulative_seconds.push(0.0);
        for window in events.windows(2) {
            let (a, b) = (&window[0], &window[1]);
            let prev = *cumulative_seconds.last().expect("primed with 0.0");
            cumulative_seconds.push(prev + segment_seconds(a, b));
        }

        Tempo::Automated(AutomatedTempo {
            points: events,
            cumulative_seconds,
        })
    }

    /// Convert a beat position to seconds from the start of the project.
    /// Beats less than zero clamp to zero.
    pub fn seconds_at(&self, beats: f64) -> f64 {
        let beats = beats.max(0.0);
        match self {
            Tempo::Constant(bpm) => beats * 60.0 / bpm,
            Tempo::Automated(at) => at.seconds_at(beats),
        }
    }
}

impl AutomatedTempo {
    fn seconds_at(&self, beats: f64) -> f64 {
        // Caller has clamped to >= 0; first point is at beat 0.
        debug_assert!(beats >= 0.0);
        debug_assert!(self.points.first().map(|p| p.beat) == Some(0.0));

        // Find the segment [points[i], points[i+1]] containing `beats`, or
        // the last segment for extrapolation.
        let last_idx = self.points.len() - 1;
        if beats >= self.points[last_idx].beat {
            // Hold last BPM past the final point.
            let extra = beats - self.points[last_idx].beat;
            return self.cumulative_seconds[last_idx] + extra * 60.0 / self.points[last_idx].bpm;
        }

        // Binary search for upper bound.
        let upper = self
            .points
            .partition_point(|p| p.beat <= beats)
            .min(last_idx);
        let lower = upper - 1;
        let a = self.points[lower];
        let b = self.points[upper];
        let target = TempoPoint {
            beat: beats,
            bpm: interpolate_bpm(&a, &b, beats),
        };
        self.cumulative_seconds[lower] + segment_seconds(&a, &target)
    }
}

/// Linearly interpolate BPM between two control points at `beat`.
fn interpolate_bpm(a: &TempoPoint, b: &TempoPoint, beat: f64) -> f64 {
    if (b.beat - a.beat).abs() < f64::EPSILON {
        return a.bpm;
    }
    let t = (beat - a.beat) / (b.beat - a.beat);
    a.bpm + t * (b.bpm - a.bpm)
}

/// Time elapsed between two tempo control points with linear interpolation.
fn segment_seconds(a: &TempoPoint, b: &TempoPoint) -> f64 {
    let dbeats = b.beat - a.beat;
    if dbeats <= 0.0 {
        return 0.0;
    }
    if (b.bpm - a.bpm).abs() < 1e-9 {
        return dbeats * 60.0 / a.bpm;
    }
    // Tempo varies linearly with beat, so dt/db = 60/T(b).
    // ∫ db / (a.bpm + slope*(b - a.beat)) = (1/slope) * ln(T(b)/T(a))
    60.0 * dbeats / (b.bpm - a.bpm) * (b.bpm / a.bpm).ln()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64, eps: f64) {
        assert!((a - b).abs() < eps, "expected {a} ≈ {b} (eps={eps})");
    }

    #[test]
    fn constant_tempo_basic() {
        let t = Tempo::Constant(120.0);
        approx(t.seconds_at(0.0), 0.0, 1e-9);
        approx(t.seconds_at(120.0), 60.0, 1e-9); // 120 beats @ 120bpm = 1 min
        approx(t.seconds_at(60.0), 30.0, 1e-9);
    }

    #[test]
    fn constant_tempo_clamps_negative() {
        let t = Tempo::Constant(120.0);
        approx(t.seconds_at(-5.0), 0.0, 1e-9);
    }

    #[test]
    fn automated_tempo_collapses_to_constant_when_single_point() {
        let t = Tempo::from_automation_events(
            vec![TempoPoint {
                beat: -63072000.0,
                bpm: 100.0,
            }],
            120.0,
        );
        match t {
            Tempo::Constant(bpm) => approx(bpm, 100.0, 1e-9),
            _ => panic!("expected constant"),
        }
    }

    #[test]
    fn automated_uses_fallback_when_no_negative_event() {
        // Only positive beats given; first should be synthesized at beat 0
        // with the fallback BPM.
        let t = Tempo::from_automation_events(
            vec![TempoPoint {
                beat: 100.0,
                bpm: 140.0,
            }],
            120.0,
        );
        // Two segments in the curve: (0, 120) → (100, 140).
        // From 0 to 100 beats with BPM linearly going 120 -> 140:
        // dt = 60 * 100 / (140 - 120) * ln(140/120) = 300 * ln(7/6)
        let expected = 300.0 * (140.0_f64 / 120.0).ln();
        approx(t.seconds_at(100.0), expected, 1e-6);
    }

    #[test]
    fn automated_constant_segment_matches_closed_form() {
        let t = Tempo::from_automation_events(
            vec![
                TempoPoint {
                    beat: -63072000.0,
                    bpm: 120.0,
                },
                TempoPoint {
                    beat: 0.0,
                    bpm: 120.0,
                },
                TempoPoint {
                    beat: 60.0,
                    bpm: 120.0,
                },
            ],
            120.0,
        );
        // Constant 120 BPM over [0, 60] beats = 30 seconds.
        approx(t.seconds_at(60.0), 30.0, 1e-9);
        approx(t.seconds_at(30.0), 15.0, 1e-9);
    }

    #[test]
    fn automated_extrapolates_past_last_point() {
        // 120 BPM held; query past final point.
        let t = Tempo::from_automation_events(
            vec![TempoPoint {
                beat: 0.0,
                bpm: 120.0,
            }],
            120.0,
        );
        // Single point collapses to Constant; should extrapolate fine.
        approx(t.seconds_at(240.0), 120.0, 1e-9);

        // With multi-point curve held at 100 BPM past last point:
        let t = Tempo::from_automation_events(
            vec![
                TempoPoint {
                    beat: 0.0,
                    bpm: 100.0,
                },
                TempoPoint {
                    beat: 100.0,
                    bpm: 100.0,
                },
            ],
            100.0,
        );
        // 0..100 beats @ 100 bpm = 60s. Then 100..200 beats @ 100 bpm = 60s.
        approx(t.seconds_at(200.0), 120.0, 1e-9);
    }

    #[test]
    fn automated_interpolates_within_segment() {
        // Single linear ramp 120 -> 140 over 100 beats.
        let t = Tempo::from_automation_events(
            vec![
                TempoPoint {
                    beat: 0.0,
                    bpm: 120.0,
                },
                TempoPoint {
                    beat: 100.0,
                    bpm: 140.0,
                },
            ],
            120.0,
        );
        // At beat 50, BPM = 130. Closed-form: 60*50/(130-120)*ln(130/120)
        // = 300 * ln(13/12).
        let expected = 300.0 * (130.0_f64 / 120.0).ln();
        approx(t.seconds_at(50.0), expected, 1e-6);
    }

    #[test]
    fn automated_dedups_identical_beat_points() {
        let t = Tempo::from_automation_events(
            vec![
                TempoPoint {
                    beat: 0.0,
                    bpm: 100.0,
                },
                TempoPoint {
                    beat: 0.0,
                    bpm: 120.0,
                },
                TempoPoint {
                    beat: 100.0,
                    bpm: 120.0,
                },
            ],
            120.0,
        );
        // After dedup: only beat 0 (last wins → 120) and beat 100 (120) remain
        // → constant 120 over the range → 100*60/120 = 50s.
        approx(t.seconds_at(100.0), 50.0, 1e-9);
    }

    #[test]
    fn interpolate_bpm_handles_zero_width_segment() {
        let a = TempoPoint {
            beat: 5.0,
            bpm: 100.0,
        };
        let b = TempoPoint {
            beat: 5.0,
            bpm: 200.0,
        };
        approx(interpolate_bpm(&a, &b, 5.0), 100.0, 1e-9);
    }

    #[test]
    fn segment_seconds_zero_for_non_positive_dbeats() {
        let a = TempoPoint {
            beat: 5.0,
            bpm: 120.0,
        };
        let b = TempoPoint {
            beat: 5.0,
            bpm: 130.0,
        };
        approx(segment_seconds(&a, &b), 0.0, 1e-9);
        let c = TempoPoint {
            beat: 4.0,
            bpm: 120.0,
        };
        approx(segment_seconds(&a, &c), 0.0, 1e-9);
    }
}
