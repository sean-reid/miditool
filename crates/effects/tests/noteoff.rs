//! No effect may orphan a note: after an arbitrary sequence of note-ons and
//! note-offs plus a flush, nothing is left sounding at the output.

use std::sync::atomic::Ordering;

use miditool_core::event::CC_SUSTAIN;
use miditool_core::{
    Effect, Event, EventBuf, EventKind, MAX_FANOUT, Node, NoteTracker, ProcCx, Sieve,
};
use miditool_effects::{
    AccentGroups, AddedValue, AggregateGate, AntiAccent, BlockedKeys, BrownianWalker, Channelize,
    ClusterAnchor, ClusterFist, ClusterKind, ComplementPad, Continuator, Continuum, ContinuumOrder,
    CrescendoShape, CrippledLooper, Delay, DensityGovernor, DurationLottery, Echo, EuclideanGate,
    FeldmanField, Just as JustIntonation, KeyDist, Klangfarben, LooseKeys, MassCrescendo,
    Mechanico, MetronomeSwarm, ModeLock, MpeParams, NegativeHarmony, NoteRoulette, OvertonePedal,
    Plr, PoissonCloud, Quantize, RegistralScatter, ResonanceHalo, Restrike, RetrogradeBuffer,
    RingMod, RowForm, RowSnap, Scordatura, ShuffleLock, ShuffleMode, SieveQuantizer, SieveSnap,
    SpectralHalo, Stutter, TDirection, Talea, Telescope, Tintinnabuli, Tonnetz, Transpose, VelDist,
    VelocityCurve, VelocityDice, VelocityInvert, VelocityRouter, WedgeMirror,
};
use proptest::prelude::*;

#[derive(Debug, Clone)]
struct Step {
    on: bool,
    ch: u8,
    key: u8,
    vel: u8,
}

/// Short sequences keep the worst-case flush (one note-off per active note
/// per stateful effect in a chain) inside one EventBuf.
fn steps() -> impl Strategy<Value = Vec<Step>> {
    prop::collection::vec(
        (any::<bool>(), 0u8..2, 0u8..128, 1u8..128).prop_map(|(on, ch, key, vel)| Step {
            on,
            ch,
            key,
            vel,
        }),
        0..32,
    )
}

fn modes() -> impl Strategy<Value = ShuffleMode> {
    prop_oneof![
        Just(ShuffleMode::Free),
        Just(ShuffleMode::WithinOctave),
        Just(ShuffleMode::WithinPitchClass),
    ]
}

fn forms() -> impl Strategy<Value = RowForm> {
    prop_oneof![
        Just(RowForm::Prime),
        Just(RowForm::Inversion),
        Just(RowForm::Retrograde),
        Just(RowForm::RetrogradeInversion),
    ]
}

fn snaps() -> impl Strategy<Value = SieveSnap> {
    prop_oneof![
        Just(SieveSnap::Nearest),
        Just(SieveSnap::Up),
        Just(SieveSnap::Down),
        Just(SieveSnap::Drop),
    ]
}

fn tdirections() -> impl Strategy<Value = TDirection> {
    prop_oneof![
        Just(TDirection::Superior),
        Just(TDirection::Inferior),
        Just(TDirection::Alternating),
    ]
}

fn plr_sequences() -> impl Strategy<Value = Vec<Plr>> {
    prop::collection::vec(prop_oneof![Just(Plr::P), Just(Plr::L), Just(Plr::R)], 0..6)
}

fn shapes() -> impl Strategy<Value = CrescendoShape> {
    prop_oneof![Just(CrescendoShape::Ramp), Just(CrescendoShape::Arch)]
}

fn cluster_kinds() -> impl Strategy<Value = ClusterKind> {
    prop_oneof![
        Just(ClusterKind::Chromatic),
        Just(ClusterKind::White),
        Just(ClusterKind::Black),
        sieves().prop_map(ClusterKind::Sieve),
    ]
}

fn anchors() -> impl Strategy<Value = ClusterAnchor> {
    prop_oneof![
        Just(ClusterAnchor::Bottom),
        Just(ClusterAnchor::Center),
        Just(ClusterAnchor::Top),
    ]
}

fn continuum_orders() -> impl Strategy<Value = ContinuumOrder> {
    prop_oneof![
        Just(ContinuumOrder::Up),
        Just(ContinuumOrder::Down),
        Just(ContinuumOrder::Played),
        Just(ContinuumOrder::Random),
    ]
}

fn rows() -> impl Strategy<Value = [u8; 12]> {
    Just((0u8..12).collect::<Vec<u8>>())
        .prop_shuffle()
        .prop_map(|row| std::array::from_fn(|i| row[i]))
}

/// A single-atom sieve `m@r`; enough to exercise every snap mode,
/// including the drop-at-the-edges paths of `Up` and `Down`.
fn sieves() -> impl Strategy<Value = Sieve> {
    (1u8..=127)
        .prop_flat_map(|m| (Just(m), 0..m))
        .prop_map(|(m, r)| Sieve::parse(&format!("{m}@{r}")).expect("a valid atom"))
}

/// Append a note-off per outstanding note-on. A stateless effect cannot end
/// notes the player never released, so its no-orphan property is stated
/// over balanced input; the stateful effects are tested on raw sequences,
/// where flush must do the cleanup.
fn balanced(steps: &[Step]) -> Vec<Step> {
    let mut counts = [[0u32; 128]; 16];
    let mut all = steps.to_vec();
    for step in steps {
        let slot = &mut counts[step.ch as usize][step.key as usize];
        *slot = if step.on {
            *slot + 1
        } else {
            slot.saturating_sub(1)
        };
    }
    for (ch, keys) in counts.iter().enumerate() {
        for (key, &count) in keys.iter().enumerate() {
            for _ in 0..count {
                all.push(Step {
                    on: false,
                    ch: ch as u8,
                    key: key as u8,
                    vel: 1,
                });
            }
        }
    }
    all
}

fn step_kind(step: &Step) -> EventKind {
    if step.on {
        EventKind::NoteOn {
            ch: step.ch,
            key: step.key,
            vel: step.vel,
        }
    } else {
        EventKind::NoteOff {
            ch: step.ch,
            key: step.key,
            vel: 0,
        }
    }
}

fn assert_no_orphans_kinds(node: &mut Node, kinds: &[EventKind]) {
    let mut tracker = NoteTracker::new();
    let cx = ProcCx::at(0);
    for (i, kind) in kinds.iter().enumerate() {
        let mut out = EventBuf::new();
        node.process(&Event::new(i as u64, *kind), &mut out, &cx);
        for ev in &out {
            tracker.observe(&ev.kind);
        }
    }
    let mut out = EventBuf::new();
    node.flush(&mut out, &cx);
    for ev in &out {
        tracker.observe(&ev.kind);
    }
    assert_eq!(tracker.active(), 0, "orphaned notes at the output");
}

fn assert_no_orphans(node: &mut Node, steps: &[Step]) {
    let kinds: Vec<EventKind> = steps.iter().map(step_kind).collect();
    assert_no_orphans_kinds(node, &kinds);
}

/// Tick-aware harness for the free-running generators: each event lands
/// at an increasing timestamp with tick calls between events and a stretch
/// of trailing ticks afterward (long enough to wake idle-triggered voices
/// and run down repeat counts), then a flush. Every note the effect
/// emitted must balance per (channel, key).
fn assert_no_orphans_ticked(node: &mut Node, steps: &[Step]) {
    fn drive(node: &mut Node, tracker: &mut NoteTracker, now: u64, ev: Option<EventKind>) {
        let cx = ProcCx::at(now);
        let mut out = EventBuf::new();
        match ev {
            Some(kind) => node.process(&Event::new(now, kind), &mut out, &cx),
            None => node.tick(now, &mut out, &cx),
        }
        for e in &out {
            tracker.observe(&e.kind);
        }
    }
    let mut tracker = NoteTracker::new();
    let mut now: u64 = 0;
    for step in steps {
        // Irregular but deterministic spacing, up to about 1.2 seconds.
        now += 1_000_000 + step.vel as u64 * 9_000_000;
        drive(node, &mut tracker, now, Some(step_kind(step)));
        now += 2_000_000 + step.key as u64 * 500_000;
        drive(node, &mut tracker, now, None);
    }
    for _ in 0..12 {
        now += 800_000_000;
        drive(node, &mut tracker, now, None);
    }
    let cx = ProcCx::at(now);
    let mut out = EventBuf::new();
    node.flush(&mut out, &cx);
    for e in &out {
        tracker.observe(&e.kind);
    }
    assert_eq!(tracker.active(), 0, "orphaned notes at the output");
}

/// Tick-aware harness for the pedal-capture generators: like
/// `assert_no_orphans_ticked`, but the capture pedal is driven through
/// the sequence (down at the start like the resonance-halo precedent, up
/// midway so a phrase is captured and playback begins, and a final
/// down/up pair so the mid-playback stop and a second capture run too),
/// with ticks interleaved throughout. Raw sequences: these effects pass
/// the player's notes through and their flush winds the pass-through
/// down alongside whatever the machine still has sounding.
fn assert_no_orphans_ticked_pedal(node: &mut Node, steps: &[Step], pedal_cc: u8) {
    fn drive(node: &mut Node, tracker: &mut NoteTracker, now: u64, ev: Option<EventKind>) {
        let cx = ProcCx::at(now);
        let mut out = EventBuf::new();
        match ev {
            Some(kind) => node.process(&Event::new(now, kind), &mut out, &cx),
            None => node.tick(now, &mut out, &cx),
        }
        for e in &out {
            tracker.observe(&e.kind);
        }
    }
    let pedal = |value: u8| EventKind::ControlChange {
        ch: 0,
        cc: pedal_cc,
        value,
    };
    let mut tracker = NoteTracker::new();
    let mut now: u64 = 0;
    drive(node, &mut tracker, now, Some(pedal(127)));
    let half = steps.len() / 2;
    for (i, step) in steps.iter().enumerate() {
        if i == half {
            now += 1_000_000;
            drive(node, &mut tracker, now, Some(pedal(0)));
        }
        now += 1_000_000 + step.vel as u64 * 9_000_000;
        drive(node, &mut tracker, now, Some(step_kind(step)));
        now += 2_000_000 + step.key as u64 * 500_000;
        drive(node, &mut tracker, now, None);
    }
    // Lift the pedal (a no-op if it already lifted midway) and let the
    // playback or loop run under trailing ticks.
    now += 1_000_000;
    drive(node, &mut tracker, now, Some(pedal(0)));
    for _ in 0..6 {
        now += 800_000_000;
        drive(node, &mut tracker, now, None);
    }
    // Stomp mid-playback: the machine must silence itself, capture
    // nothing, and stay quiet after the empty pedal-up.
    now += 1_000_000;
    drive(node, &mut tracker, now, Some(pedal(127)));
    for _ in 0..3 {
        now += 800_000_000;
        drive(node, &mut tracker, now, None);
    }
    now += 1_000_000;
    drive(node, &mut tracker, now, Some(pedal(0)));
    for _ in 0..6 {
        now += 800_000_000;
        drive(node, &mut tracker, now, None);
    }
    let cx = ProcCx::at(now);
    let mut out = EventBuf::new();
    node.flush(&mut out, &cx);
    for e in &out {
        tracker.observe(&e.kind);
    }
    assert_eq!(tracker.active(), 0, "orphaned notes at the output");
}

fn leaf(fx: impl Effect + 'static) -> Node {
    Node::Leaf(Box::new(fx))
}

proptest! {
    #[test]
    fn transpose_no_orphans(steps in steps(), semis in -140i16..=140) {
        assert_no_orphans(&mut leaf(Transpose::new(semis)), &steps);
    }

    #[test]
    fn shuffle_lock_no_orphans(
        steps in steps(),
        seed: u64,
        lo in 0u8..128,
        hi in 0u8..128,
        mode in modes(),
    ) {
        assert_no_orphans(&mut leaf(ShuffleLock::new(seed, lo, hi, mode)), &steps);
    }

    #[test]
    fn loose_keys_uniform_no_orphans(
        steps in steps(),
        seed: u64,
        lo in 0u8..128,
        hi in 0u8..128,
    ) {
        let fx = LooseKeys::new(seed, KeyDist::Uniform { lo, hi });
        assert_no_orphans(&mut leaf(fx), &steps);
    }

    #[test]
    fn loose_keys_gaussian_no_orphans(steps in steps(), seed: u64, sigma in 0.0f32..60.0) {
        let fx = LooseKeys::new(seed, KeyDist::Gaussian { sigma });
        assert_no_orphans(&mut leaf(fx), &steps);
    }

    #[test]
    fn velocity_curve_no_orphans(steps in steps(), gamma in 0.1f32..4.0) {
        let fx = VelocityCurve { gamma, floor: 10, ceiling: 120 };
        assert_no_orphans(&mut leaf(fx), &balanced(&steps));
    }

    #[test]
    fn channelize_no_orphans(steps in steps(), ch in 0u8..16) {
        assert_no_orphans(&mut leaf(Channelize { ch }), &balanced(&steps));
    }

    #[test]
    fn chain_no_orphans(steps in steps(), seed: u64, semis in -24i16..=24, mode in modes()) {
        let mut node = Node::Chain(vec![
            leaf(Channelize { ch: 3 }),
            leaf(Transpose::new(semis)),
            leaf(ShuffleLock::new(seed, 24, 96, mode)),
            leaf(LooseKeys::new(seed, KeyDist::Gaussian { sigma: 12.0 })),
            leaf(VelocityCurve { gamma: 1.5, floor: 5, ceiling: 127 }),
        ]);
        assert_no_orphans(&mut node, &steps);
    }

    #[test]
    fn delay_no_orphans(steps in steps(), delta in 0u64..=2_000_000_000) {
        assert_no_orphans(&mut leaf(Delay::new(delta)), &balanced(&steps));
    }

    #[test]
    fn echo_no_orphans(
        steps in steps(),
        repeats in 0u8..=20,
        delta in 0u64..=1_000_000_000,
        decay in 0.0f32..=1.5,
        transpose in -140i16..=140,
    ) {
        let fx = Echo::new(repeats, delta, decay, transpose);
        assert_no_orphans(&mut leaf(fx), &balanced(&steps));
    }

    #[test]
    fn restrike_no_orphans(
        steps in steps(),
        seed: u64,
        interval in 0u64..=1_000_000_000,
        jitter in 0.0f32..=2.0,
        decay in 0.0f32..=1.5,
        floor in 0u8..=127,
        max_repeats in 0u8..=30,
    ) {
        let fx = Restrike::new(seed, interval, jitter, decay, floor, max_repeats);
        assert_no_orphans(&mut leaf(fx), &balanced(&steps));
    }

    #[test]
    fn stutter_no_orphans(
        steps in steps(),
        repeats in 0u8..=30,
        gap in 0u64..=1_000_000_000,
        curve in 0.1f32..=5.0,
    ) {
        let fx = Stutter::new(repeats, gap, curve);
        assert_no_orphans(&mut leaf(fx), &balanced(&steps));
    }

    #[test]
    fn registral_scatter_no_orphans(
        steps in steps(),
        seed: u64,
        lo in 0u8..128,
        hi in 0u8..128,
    ) {
        assert_no_orphans(&mut leaf(RegistralScatter::new(seed, lo, hi)), &steps);
    }

    #[test]
    fn wedge_mirror_no_orphans(
        steps in steps(),
        seed: u64,
        axis in 0u8..128,
        probability in 0.0f32..=1.0,
    ) {
        assert_no_orphans(&mut leaf(WedgeMirror::new(axis, probability, seed)), &steps);
    }

    #[test]
    fn blocked_keys_no_orphans(
        steps in steps(),
        items in prop::collection::vec(0u8..128, 0..16),
        by_class: bool,
    ) {
        assert_no_orphans(&mut leaf(BlockedKeys::new(&items, by_class)), &steps);
    }

    #[test]
    fn klangfarben_no_orphans(
        steps in steps(),
        seed: u64,
        channels in prop::collection::vec(0u8..16, 1..8),
        random: bool,
    ) {
        assert_no_orphans(&mut leaf(Klangfarben::new(&channels, random, seed)), &steps);
    }

    #[test]
    fn ring_mod_no_orphans(
        steps in steps(),
        carrier in 0u8..128,
        sum: bool,
        diff: bool,
        dry: bool,
    ) {
        assert_no_orphans(&mut leaf(RingMod::new(carrier, sum, diff, dry)), &steps);
    }

    #[test]
    fn telescope_no_orphans(
        steps in steps(),
        factor in -4.0f32..=4.0,
        reference in 0u8..128,
    ) {
        assert_no_orphans(&mut leaf(Telescope::new(factor, reference)), &steps);
    }

    #[test]
    fn row_snap_no_orphans(
        steps in steps(),
        row in rows(),
        form in forms(),
        transpose in -12i8..=12,
    ) {
        assert_no_orphans(&mut leaf(RowSnap::new(row, form, transpose)), &steps);
    }

    #[test]
    fn aggregate_gate_no_orphans(steps in steps(), leak in 0.0f32..=1.0, seed: u64) {
        assert_no_orphans(&mut leaf(AggregateGate::new(leak, seed)), &steps);
    }

    #[test]
    fn sieve_quantizer_no_orphans(steps in steps(), sieve in sieves(), snap in snaps()) {
        assert_no_orphans(&mut leaf(SieveQuantizer::new(sieve, snap)), &steps);
    }

    // The passed-through original needs the player's off; the grains are
    // self-contained pairs, so the tracker still ends at zero.
    #[test]
    fn poisson_cloud_no_orphans(
        steps in steps(),
        seed: u64,
        density in 0.0f32..=1_000.0,
        duration in 0u64..=2_000_000_000,
        pitch_sigma in 0.0f32..=60.0,
        vel_sigma in 0.0f32..=60.0,
        max_grains in 0u8..=30,
    ) {
        let fx = PoissonCloud::new(seed, density, duration, pitch_sigma, vel_sigma, max_grains);
        assert_no_orphans(&mut leaf(fx), &balanced(&steps));
    }

    #[test]
    fn note_roulette_no_orphans(
        steps in steps(),
        seed: u64,
        pass in 0.0f32..=2.0,
        replace in 0.0f32..=2.0,
        lo in 0u8..128,
        hi in 0u8..128,
    ) {
        assert_no_orphans(&mut leaf(NoteRoulette::new(seed, pass, replace, lo, hi)), &steps);
    }

    #[test]
    fn velocity_dice_uniform_no_orphans(
        steps in steps(),
        seed: u64,
        lo in 0u8..128,
        hi in 0u8..128,
    ) {
        let fx = VelocityDice::new(seed, VelDist::Uniform { lo, hi });
        assert_no_orphans(&mut leaf(fx), &balanced(&steps));
    }

    #[test]
    fn velocity_dice_gaussian_no_orphans(steps in steps(), seed: u64, sigma in 0.0f32..=60.0) {
        let fx = VelocityDice::new(seed, VelDist::Gaussian { sigma });
        assert_no_orphans(&mut leaf(fx), &balanced(&steps));
    }

    // Raw sequences on purpose: the lottery swallows the player's offs and
    // every on carries its own drawn off, so the tracker ends at zero even
    // over unbalanced input.
    #[test]
    fn duration_lottery_no_orphans(
        steps in steps(),
        seed: u64,
        mean in 0u64..=2_000_000_000,
        min in 0u64..=1_000_000,
        max in 0u64..=2_000_000_000,
        uniform: bool,
    ) {
        let fx = DurationLottery::new(seed, mean, min, max, uniform);
        assert_no_orphans(&mut leaf(fx), &steps);
    }

    #[test]
    fn density_governor_no_orphans(
        steps in steps(),
        seed: u64,
        target in 0.0f32..=200.0,
        window in 0u64..=2_000_000_000,
    ) {
        assert_no_orphans(&mut leaf(DensityGovernor::new(seed, target, window)), &steps);
    }

    // Sequences stay short: flush emits up to 12 offs per active note, and
    // that total must fit one EventBuf.
    #[test]
    fn cluster_fist_no_orphans(
        steps in steps(),
        kind in cluster_kinds(),
        width in 0u8..=16,
        anchor in anchors(),
        rolloff in 0.0f32..=1.5,
    ) {
        let fx = ClusterFist::new(kind, width, anchor, rolloff);
        assert_no_orphans(&mut leaf(fx), &steps[..steps.len().min(10)]);
    }

    // The pedal goes down first so halos actually deposit; they are
    // self-contained pairs, so balanced input still ends at zero.
    #[test]
    fn resonance_halo_no_orphans(
        steps in steps(),
        width in 0u8..=8,
        level in 0.0f32..=2.0,
        decay in 0u64..=1_000_000_000,
        sieve in prop::option::of(sieves()),
    ) {
        let mut kinds = vec![
            EventKind::ControlChange { ch: 0, cc: CC_SUSTAIN, value: 127 },
            EventKind::ControlChange { ch: 1, cc: CC_SUSTAIN, value: 127 },
        ];
        kinds.extend(balanced(&steps).iter().map(step_kind));
        let fx = ResonanceHalo::new(width, level, decay, sieve);
        assert_no_orphans_kinds(&mut leaf(fx), &kinds);
    }

    // Echo repeats stay small here: Transpose's flush emits one note-off
    // per note it tracks, up to (1 + repeats) keys per input note-on, and
    // that total must fit one EventBuf.
    #[test]
    fn echo_into_transpose_no_orphans(
        steps in steps(),
        semis in -24i16..=24,
        transpose in -12i16..=12,
    ) {
        let mut node = Node::Chain(vec![
            leaf(Echo::new(2, 1_000_000, 0.8, transpose)),
            leaf(Transpose::new(semis)),
        ]);
        assert_no_orphans(&mut node, &steps);
    }

    // Raw sequences: dropped note-ons swallow their offs, and deferred
    // ons emit their offs late but balanced; the harness checks balance,
    // not timing.
    #[test]
    fn euclidean_gate_no_orphans(
        steps in steps(),
        k in 0u8..=70,
        n in 0u8..=70,
        rotation: u8,
        pulse in 0u64..=2_000_000_000,
        defer: bool,
    ) {
        let fx = EuclideanGate::new(k, n, rotation, pulse, defer);
        assert_no_orphans(&mut leaf(fx), &steps);
    }

    #[test]
    fn quantize_no_orphans(
        steps in steps(),
        grid in 0u64..=2_000_000_000,
        strength in 0.0f32..=1.5,
    ) {
        assert_no_orphans(&mut leaf(Quantize::new(grid, strength)), &steps);
    }

    // Raw sequences on purpose: the talea swallows the player's offs and
    // every on carries its own scheduled off, like the duration lottery.
    #[test]
    fn talea_no_orphans(
        steps in steps(),
        durations in prop::collection::vec(0u64..=2_000_000_000, 0..40),
    ) {
        assert_no_orphans(&mut leaf(Talea::new(&durations)), &steps);
    }

    #[test]
    fn added_value_no_orphans(
        steps in steps(),
        seed: u64,
        unit in 0u64..=2_000_000_000,
        extend_p in 0.0f32..=1.5,
        defer_p in 0.0f32..=1.5,
    ) {
        let fx = AddedValue::new(seed, unit, extend_p, defer_p);
        assert_no_orphans(&mut leaf(fx), &steps);
    }

    #[test]
    fn accent_groups_no_orphans(
        steps in steps(),
        groups in prop::collection::vec(0u8..=20, 0..6),
        accent: u8,
        rest: u8,
    ) {
        let fx = AccentGroups::new(&groups, accent, rest);
        assert_no_orphans(&mut leaf(fx), &balanced(&steps));
    }

    #[test]
    fn feldman_field_no_orphans(
        steps in steps(),
        seed: u64,
        floor: u8,
        ceiling: u8,
        jitter: u8,
    ) {
        let fx = FeldmanField::new(seed, floor, ceiling, jitter);
        assert_no_orphans(&mut leaf(fx), &balanced(&steps));
    }

    #[test]
    fn velocity_invert_no_orphans(steps in steps(), pivot: u8) {
        assert_no_orphans(&mut leaf(VelocityInvert::new(pivot)), &balanced(&steps));
    }

    #[test]
    fn velocity_router_no_orphans(
        steps in steps(),
        low: u8,
        high: u8,
        soft_ch: u8,
        mid_ch: u8,
        loud_ch: u8,
    ) {
        let fx = VelocityRouter::new(low, high, soft_ch, mid_ch, loud_ch);
        assert_no_orphans(&mut leaf(fx), &steps);
    }

    #[test]
    fn anti_accent_no_orphans(
        steps in steps(),
        seed: u64,
        level: u8,
        every in 0u64..=10_000_000_000,
    ) {
        let fx = AntiAccent::new(seed, level, every);
        assert_no_orphans(&mut leaf(fx), &balanced(&steps));
    }

    #[test]
    fn mass_crescendo_no_orphans(
        steps in steps(),
        period in 0u64..=10_000_000_000,
        depth in 0.0f32..=1.5,
        shape in shapes(),
    ) {
        let fx = MassCrescendo::new(period, depth, shape);
        assert_no_orphans(&mut leaf(fx), &balanced(&steps));
    }

    // Raw sequences: the component pair covers the T-voice, and an orphan
    // note-off releases the played key alone.
    #[test]
    fn tintinnabuli_no_orphans(
        steps in steps(),
        root_pc: u8,
        minor: bool,
        position in 0u8..=4,
        direction in tdirections(),
        level in 0.0f32..=1.5,
    ) {
        let fx = Tintinnabuli::new(root_pc, minor, position, direction, level);
        assert_no_orphans(&mut leaf(fx), &steps);
    }

    #[test]
    fn mode_lock_no_orphans(
        steps in steps(),
        mode in 0u8..=9,
        transposition: u8,
        snap in snaps(),
    ) {
        assert_no_orphans(&mut leaf(ModeLock::new(mode, transposition, snap)), &steps);
    }

    #[test]
    fn negative_harmony_no_orphans(
        steps in steps(),
        tonic_pc: u8,
        add: bool,
        level in 0.0f32..=1.5,
    ) {
        assert_no_orphans(&mut leaf(NegativeHarmony::new(tonic_pc, add, level)), &steps);
    }

    // Raw sequences: orphan note-offs are dropped and flush releases every
    // remembered set. Sequences stay short: flush emits up to 4 offs per
    // active note, and that total must fit one EventBuf.
    #[test]
    fn tonnetz_no_orphans(
        steps in steps(),
        root_pc: u8,
        minor: bool,
        sequence in plr_sequences(),
        lo in 0u8..128,
        hi in 0u8..128,
        include_played: bool,
    ) {
        let fx = Tonnetz::new(root_pc, minor, &sequence, lo, hi, include_played);
        assert_no_orphans(&mut leaf(fx), &steps[..steps.len().min(24)]);
    }

    // Raw sequences: the pad diffs on every on and off, and flush releases
    // the pad plus one off per outstanding pass-through note-on.
    #[test]
    fn complement_pad_no_orphans(
        steps in steps(),
        lo in 0u8..128,
        hi in 0u8..128,
        vel: u8,
    ) {
        assert_no_orphans(&mut leaf(ComplementPad::new(lo, hi, vel)), &steps);
    }

    // The tick-aware generators run on raw sequences: they consume the
    // player's notes (Continuator excepted, whose flush winds down the
    // pass-through) and every voice they start is either a self-contained
    // pair or released from their own bookkeeping by tick or flush.
    #[test]
    fn continuum_no_orphans(
        steps in steps(),
        seed: u64,
        rate in 0.0f32..=60.0,
        gate in 0.0f32..=2.0,
        order in continuum_orders(),
    ) {
        let fx = Continuum::new(rate, order, gate, seed);
        assert_no_orphans_ticked(&mut leaf(fx), &steps);
    }

    #[test]
    fn metronome_swarm_no_orphans(
        steps in steps(),
        seed: u64,
        bpm_lo in 0.0f32..=500.0,
        bpm_hi in 0.0f32..=500.0,
        max_repeats in 0u8..=80,
        fade in 0.0f32..=1.5,
    ) {
        let fx = MetronomeSwarm::new(seed, bpm_lo, bpm_hi, max_repeats, fade);
        assert_no_orphans_ticked(&mut leaf(fx), &steps);
    }

    #[test]
    fn brownian_walker_no_orphans(
        steps in steps(),
        seed: u64,
        interval in 0u64..=300_000_000,
        sigma in 0.0f32..=20.0,
        lo in 0u8..128,
        hi in 0u8..128,
    ) {
        let fx = BrownianWalker::new(seed, interval, sigma, lo, hi);
        assert_no_orphans_ticked(&mut leaf(fx), &steps);
    }

    #[test]
    fn mechanico_no_orphans(
        steps in steps(),
        seed: u64,
        pulse in 0u64..=400_000_000,
        repeats in 0u8..=80,
        jam in 0.0f32..=1.0,
    ) {
        let fx = Mechanico::new(pulse, repeats, jam, seed);
        assert_no_orphans_ticked(&mut leaf(fx), &steps);
    }

    #[test]
    fn continuator_no_orphans(
        steps in steps(),
        seed: u64,
        idle in 0u64..=3_000_000_000,
        max_notes in 0u16..=1200,
    ) {
        let fx = Continuator::new(seed, idle, max_notes);
        assert_no_orphans_ticked(&mut leaf(fx), &steps);
    }

    // The pedal-capture generators pass the player's notes through, so
    // they run on raw sequences with the pedal driven around them; every
    // machine voice is released from their bookkeeping by tick,
    // pedal-down, or flush, and flush winds down the pass-through.
    #[test]
    fn crippled_looper_no_orphans(
        steps in steps(),
        seed: u64,
        max_notes in 0u8..=64,
    ) {
        let fx = CrippledLooper::new(seed, CC_SUSTAIN, max_notes);
        assert_no_orphans_ticked_pedal(&mut leaf(fx), &steps, CC_SUSTAIN);
    }

    #[test]
    fn retrograde_buffer_no_orphans(
        steps in steps(),
        speed in 0.0f32..=8.0,
    ) {
        let fx = RetrogradeBuffer::new(CC_SUSTAIN, speed);
        assert_no_orphans_ticked_pedal(&mut leaf(fx), &steps, CC_SUSTAIN);
    }

    // Raw sequences for the MPE effects: the pool's steal-offs, the
    // per-note records, and the flush (dry offs, pool offs, bend resets)
    // must balance every note-on the pool or the dry path emitted. The
    // tracker sees pool note-ons and note-offs as ordinary events and
    // ignores the pitch bends.
    #[test]
    fn spectral_halo_no_orphans(
        steps in steps(),
        partials in 0u8..=12,
        rolloff in 0.0f32..=1.5,
        stretch in 0.0f32..=4.0,
        lo in 0u8..16,
        hi in 0u8..16,
        bend_range in 1.0f32..=96.0,
    ) {
        let fx = SpectralHalo::new(partials, rolloff, stretch, MpeParams { lo, hi, bend_range });
        assert_no_orphans(&mut leaf(fx), &steps);
    }

    #[test]
    fn just_no_orphans(
        steps in steps(),
        root_pc: u8,
        lo in 0u8..16,
        hi in 0u8..16,
        bend_range in 1.0f32..=96.0,
    ) {
        let fx = JustIntonation::new(root_pc, MpeParams { lo, hi, bend_range });
        assert_no_orphans(&mut leaf(fx), &steps);
    }

    #[test]
    fn scordatura_no_orphans(
        steps in steps(),
        cents in prop::array::uniform12(-200i16..=200),
        lo in 0u8..16,
        hi in 0u8..16,
        bend_range in 1.0f32..=96.0,
    ) {
        let fx = Scordatura::new(cents, MpeParams { lo, hi, bend_range });
        assert_no_orphans(&mut leaf(fx), &steps);
    }

    // The pedal goes down on both input channels first and lifts on
    // channel 0 midway, so notes snap, pass dry, and cross a pedal move
    // between their on and off; the per-note records keep every path
    // balanced.
    #[test]
    fn overtone_pedal_no_orphans(
        steps in steps(),
        fundamental in 0u8..128,
        max_partial in 0u8..=40,
        lo in 0u8..16,
        hi in 0u8..16,
        bend_range in 1.0f32..=96.0,
    ) {
        let mut kinds = vec![
            EventKind::ControlChange { ch: 0, cc: CC_SUSTAIN, value: 127 },
            EventKind::ControlChange { ch: 1, cc: CC_SUSTAIN, value: 127 },
        ];
        let half = steps.len() / 2;
        kinds.extend(steps[..half].iter().map(step_kind));
        kinds.push(EventKind::ControlChange { ch: 0, cc: CC_SUSTAIN, value: 0 });
        kinds.extend(steps[half..].iter().map(step_kind));
        let fx = OvertonePedal::new(fundamental, max_partial, MpeParams { lo, hi, bend_range });
        assert_no_orphans_kinds(&mut leaf(fx), &kinds);
    }
}

/// Chain-stage truncation must never split a self-contained on/off pair.
/// Echo's transpose gives each copy its own key, so a per-key balance
/// shows exactly which pairs would have been split: a stutter re-attack
/// whose off was truncated away would leave its key with more ons than
/// offs, a note stuck forever.
#[test]
fn chain_truncation_never_orphans_a_note_on() {
    let mut node = Node::Chain(vec![
        leaf(Echo::new(16, 1_000_000, 0.9, 1)),
        leaf(Stutter::new(24, 10_000, 1.0)),
    ]);
    fn observe(net: &mut [i32; 128], out: &EventBuf) {
        for ev in out {
            match ev.kind {
                EventKind::NoteOn { key, .. } => net[key as usize] += 1,
                EventKind::NoteOff { key, .. } => net[key as usize] -= 1,
                _ => {}
            }
        }
    }
    let cx = ProcCx::at(0);
    let mut net = [0i32; 128];
    // 17 echo copies each fanned out 49-fold by stutter: far past one
    // EventBuf, so the chain stage must truncate.
    let mut out = EventBuf::new();
    node.process(
        &Event::new(
            0,
            EventKind::NoteOn {
                ch: 0,
                key: 60,
                vel: 100,
            },
        ),
        &mut out,
        &cx,
    );
    observe(&mut net, &out);
    assert!(
        cx.dropped.load(Ordering::Relaxed) > 0,
        "the test must exercise truncation"
    );
    for (key, &n) in net.iter().enumerate() {
        assert!(
            (0..=1).contains(&n),
            "key {key}: net {n} after the note-on (a split pair)"
        );
    }
    // The player's note-off fans out only 17-fold: every surviving
    // sustained copy ends. A leftover positive balance is a stuck note.
    let mut out = EventBuf::new();
    node.process(
        &Event::new(
            100_000_000,
            EventKind::NoteOff {
                ch: 0,
                key: 60,
                vel: 0,
            },
        ),
        &mut out,
        &cx,
    );
    observe(&mut net, &out);
    for (key, &n) in net.iter().enumerate() {
        assert!(n <= 0, "key {key}: net {n} after the note-off (stuck note)");
    }
}

/// The widest configuration of each time-based effect, fed the loudest
/// note-on, must fan out within one EventBuf and drop nothing.
#[test]
fn worst_case_fanout_fits_the_buffer() {
    let cases: Vec<(&str, Box<dyn Effect>, usize)> = vec![
        ("delay", Box::new(Delay::new(u64::MAX)), 1),
        ("echo", Box::new(Echo::new(u8::MAX, 1, 1.0, 0)), 1 + 16),
        (
            "restrike",
            Box::new(Restrike::new(0, 1, 0.9, 1.0, 0, u8::MAX)),
            1 + 2 * 24,
        ),
        (
            "stutter",
            Box::new(Stutter::new(u8::MAX, 1, 4.0)),
            1 + 2 * 24,
        ),
    ];
    let on = Event::new(
        0,
        EventKind::NoteOn {
            ch: 0,
            key: 60,
            vel: 127,
        },
    );
    for (name, mut fx, expected) in cases {
        let cx = ProcCx::at(0);
        let mut out = EventBuf::new();
        fx.process(&on, &mut out, &cx);
        assert_eq!(out.len(), expected, "{name}: fanout");
        assert!(out.len() <= MAX_FANOUT, "{name}: over MAX_FANOUT");
        assert_eq!(cx.dropped.load(Ordering::Relaxed), 0, "{name}: dropped");
    }
}

/// The widest configuration of each stochastic and cluster effect, fed the
/// loudest note-on, must fan out within one EventBuf and drop nothing.
#[test]
fn stochastic_worst_case_fanout_fits_the_buffer() {
    let on = Event::new(
        0,
        EventKind::NoteOn {
            ch: 0,
            key: 60,
            vel: 127,
        },
    );
    let cases: Vec<(&str, Box<dyn Effect>, usize)> = vec![
        (
            "note_roulette",
            Box::new(NoteRoulette::new(0, 1.0, 0.0, 0, 127)),
            1,
        ),
        (
            "velocity_dice",
            Box::new(VelocityDice::new(0, VelDist::Uniform { lo: 1, hi: 127 })),
            1,
        ),
        (
            "duration_lottery",
            Box::new(DurationLottery::new(0, 1, 1, u64::MAX, false)),
            2,
        ),
        (
            "density_governor",
            Box::new(DensityGovernor::new(0, 0.0, u64::MAX)),
            1,
        ),
        (
            "cluster_fist",
            Box::new(ClusterFist::new(
                ClusterKind::Chromatic,
                u8::MAX,
                ClusterAnchor::Center,
                0.9,
            )),
            12,
        ),
    ];
    for (name, mut fx, expected) in cases {
        let cx = ProcCx::at(0);
        let mut out = EventBuf::new();
        fx.process(&on, &mut out, &cx);
        assert_eq!(out.len(), expected, "{name}: fanout");
        assert!(out.len() <= MAX_FANOUT, "{name}: over MAX_FANOUT");
        assert_eq!(cx.dropped.load(Ordering::Relaxed), 0, "{name}: dropped");
    }

    // A retrigger of the widest cluster cuts 12 members and strikes 12.
    let mut fx = ClusterFist::new(ClusterKind::Chromatic, 12, ClusterAnchor::Center, 0.9);
    let cx = ProcCx::at(0);
    let mut out = EventBuf::new();
    fx.process(&on, &mut out, &cx);
    out.clear();
    fx.process(&on, &mut out, &cx);
    assert_eq!(out.len(), 24, "cluster_fist retrigger fanout");
    assert_eq!(cx.dropped.load(Ordering::Relaxed), 0);

    // The widest halo under the pedal: the passed note plus 12 pairs.
    let mut fx = ResonanceHalo::new(6, 1.0, u64::MAX, None);
    let cx = ProcCx::at(0);
    let mut out = EventBuf::new();
    let pedal = EventKind::ControlChange {
        ch: 0,
        cc: CC_SUSTAIN,
        value: 127,
    };
    fx.process(&Event::new(0, pedal), &mut out, &cx);
    out.clear();
    fx.process(&on, &mut out, &cx);
    assert_eq!(out.len(), 1 + 4 * 6, "resonance_halo fanout");
    assert_eq!(cx.dropped.load(Ordering::Relaxed), 0);

    // The densest, longest cloud: the count is seed-dependent but bounded
    // by 1 + 2 * max_grains, and an effectively unthinned cloud keeps
    // every arrival.
    let mut fx = PoissonCloud::new(0, 10_000.0, u64::MAX, 0.0, 0.0, u8::MAX);
    let cx = ProcCx::at(0);
    let mut out = EventBuf::new();
    fx.process(&on, &mut out, &cx);
    assert_eq!(out.len(), 1 + 2 * 24, "poisson_cloud fanout");
    assert!(out.len() <= MAX_FANOUT, "poisson_cloud: over MAX_FANOUT");
    assert_eq!(
        cx.dropped.load(Ordering::Relaxed),
        0,
        "poisson_cloud: dropped"
    );
}

/// The time, rhythm, and dynamics effects, fed a note-on and then its
/// worst case (a retrigger of the same key, which adds a cut where the
/// effect tracks notes), must fan out within one EventBuf and drop
/// nothing.
#[test]
fn time_and_dynamics_worst_case_fanout_fits_the_buffer() {
    let cases: Vec<(&str, Box<dyn Effect>, usize, usize)> = vec![
        (
            "euclidean_gate",
            Box::new(EuclideanGate::new(1, 2, 0, u64::MAX, true)),
            1,
            2,
        ),
        ("quantize", Box::new(Quantize::new(u64::MAX, 1.0)), 1, 2),
        ("talea", Box::new(Talea::new(&[u64::MAX])), 2, 2),
        (
            "added_value",
            Box::new(AddedValue::new(0, u64::MAX, 1.0, 1.0)),
            1,
            2,
        ),
        (
            "accent_groups",
            Box::new(AccentGroups::new(&[16], 127, 1)),
            1,
            1,
        ),
        (
            "feldman_field",
            Box::new(FeldmanField::new(0, 1, 127, 20)),
            1,
            1,
        ),
        ("velocity_invert", Box::new(VelocityInvert::new(127)), 1, 1),
        (
            "velocity_router",
            Box::new(VelocityRouter::new(1, 127, 0, 1, 2)),
            1,
            2,
        ),
        (
            "anti_accent",
            Box::new(AntiAccent::new(0, 1, u64::MAX)),
            1,
            1,
        ),
        (
            "mass_crescendo",
            Box::new(MassCrescendo::new(u64::MAX, 1.0, CrescendoShape::Arch)),
            1,
            1,
        ),
    ];
    let on = Event::new(
        0,
        EventKind::NoteOn {
            ch: 0,
            key: 60,
            vel: 127,
        },
    );
    for (name, mut fx, first, retrigger) in cases {
        let cx = ProcCx::at(0);
        let mut out = EventBuf::new();
        fx.process(&on, &mut out, &cx);
        assert_eq!(out.len(), first, "{name}: note-on fanout");
        out.clear();
        fx.process(&on, &mut out, &cx);
        assert_eq!(out.len(), retrigger, "{name}: retrigger fanout");
        assert!(out.len() <= MAX_FANOUT, "{name}: over MAX_FANOUT");
        assert_eq!(cx.dropped.load(Ordering::Relaxed), 0, "{name}: dropped");
    }
}

/// The widest configuration of each harmonizer, fed a note-on and then a
/// retrigger of the same key, must fan out within one EventBuf and drop
/// nothing.
#[test]
fn harmonizer_worst_case_fanout_fits_the_buffer() {
    // Key 61 keeps the played key out of every triad here, so tonnetz
    // with include_played reaches its full four keys.
    let on = Event::new(
        0,
        EventKind::NoteOn {
            ch: 0,
            key: 61,
            vel: 127,
        },
    );
    let cases: Vec<(&str, Box<dyn Effect>, usize, usize)> = vec![
        (
            "tintinnabuli",
            Box::new(Tintinnabuli::new(0, false, 2, TDirection::Alternating, 1.0)),
            2,
            2 + 2,
        ),
        (
            "mode_lock",
            Box::new(ModeLock::new(2, 0, SieveSnap::Nearest)),
            1,
            2,
        ),
        (
            "negative_harmony",
            Box::new(NegativeHarmony::new(0, true, 1.0)),
            2,
            2 + 2,
        ),
        (
            "tonnetz",
            Box::new(Tonnetz::new(0, false, &[Plr::P], 0, 127, true)),
            4,
            4 + 4,
        ),
    ];
    for (name, mut fx, first, retrigger) in cases {
        let cx = ProcCx::at(0);
        let mut out = EventBuf::new();
        fx.process(&on, &mut out, &cx);
        assert_eq!(out.len(), first, "{name}: note-on fanout");
        out.clear();
        fx.process(&on, &mut out, &cx);
        assert_eq!(out.len(), retrigger, "{name}: retrigger fanout");
        assert!(out.len() <= MAX_FANOUT, "{name}: over MAX_FANOUT");
        assert_eq!(cx.dropped.load(Ordering::Relaxed), 0, "{name}: dropped");
    }

    // The pad's widest moves: the first held note wakes 11 pad notes and
    // releasing it retires them, 12 events each way with the
    // pass-through.
    let mut fx = ComplementPad::new(0, 127, 100);
    let cx = ProcCx::at(0);
    let mut out = EventBuf::new();
    fx.process(&on, &mut out, &cx);
    assert_eq!(out.len(), 1 + 11, "complement_pad note-on fanout");
    out.clear();
    fx.process(
        &Event::new(
            1,
            EventKind::NoteOff {
                ch: 0,
                key: 61,
                vel: 0,
            },
        ),
        &mut out,
        &cx,
    );
    assert_eq!(out.len(), 1 + 11, "complement_pad note-off fanout");
    assert_eq!(
        cx.dropped.load(Ordering::Relaxed),
        0,
        "complement_pad: dropped"
    );
}

/// The widest configuration of each MPE microtonal effect, fed a note-on
/// and then a retrigger of the same key, must fan out within one EventBuf
/// and drop nothing. Each pool voice is two events (bend then on), so the
/// halo's worst note-on is 1 dry note plus 7 voices, 15 events; the
/// single-voice effects emit 2, plus the cut(s) on retrigger.
#[test]
fn mpe_worst_case_fanout_fits_the_buffer() {
    let mpe = MpeParams {
        lo: 1,
        hi: 15,
        bend_range: 48.0,
    };
    let on = Event::new(
        0,
        EventKind::NoteOn {
            ch: 0,
            key: 60,
            vel: 127,
        },
    );
    let cases: Vec<(&str, Box<dyn Effect>, usize, usize)> = vec![
        (
            "spectral_halo",
            Box::new(SpectralHalo::new(8, 1.0, 1.0, mpe)),
            15,
            8 + 15,
        ),
        ("just", Box::new(JustIntonation::new(0, mpe)), 2, 1 + 2),
        (
            "scordatura",
            Box::new(Scordatura::new([50; 12], mpe)),
            2,
            1 + 2,
        ),
    ];
    for (name, mut fx, first, retrigger) in cases {
        let cx = ProcCx::at(0);
        let mut out = EventBuf::new();
        fx.process(&on, &mut out, &cx);
        assert_eq!(out.len(), first, "{name}: note-on fanout");
        out.clear();
        fx.process(&on, &mut out, &cx);
        assert_eq!(out.len(), retrigger, "{name}: retrigger fanout");
        assert!(out.len() <= MAX_FANOUT, "{name}: over MAX_FANOUT");
        assert_eq!(cx.dropped.load(Ordering::Relaxed), 0, "{name}: dropped");
    }

    // The overtone pedal snaps only under the pedal: key 60 is one octave
    // over the fundamental, exactly partial 2.
    let mut fx = OvertonePedal::new(48, 32, mpe);
    let cx = ProcCx::at(0);
    let mut out = EventBuf::new();
    let pedal = EventKind::ControlChange {
        ch: 0,
        cc: CC_SUSTAIN,
        value: 127,
    };
    fx.process(&Event::new(0, pedal), &mut out, &cx);
    out.clear();
    fx.process(&on, &mut out, &cx);
    assert_eq!(out.len(), 2, "overtone_pedal: note-on fanout");
    out.clear();
    fx.process(&on, &mut out, &cx);
    assert_eq!(out.len(), 1 + 2, "overtone_pedal: retrigger fanout");
    assert_eq!(
        cx.dropped.load(Ordering::Relaxed),
        0,
        "overtone_pedal: dropped"
    );
}
