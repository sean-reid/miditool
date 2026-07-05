//! End-to-end tests for the public parsing API: the shipped examples, the
//! documented defaults, and the validation errors.

use miditool_config::{
    Config, EffectSpec, OutputSpec, SceneSpec, ShuffleMode, TimeSpec, parse_str,
};

fn parse(text: &str) -> Config {
    parse_str("test.kdl", text).expect("config should parse")
}

fn parse_err(text: &str) -> String {
    parse_str("test.kdl", text)
        .expect_err("config should not parse")
        .to_string()
}

/// Parse a bare-style config and return the implicit "main" scene's chain.
fn parse_chain(text: &str) -> Vec<EffectSpec> {
    let mut config = parse(text);
    assert_eq!(config.scenes.len(), 1, "bare effects lower to one scene");
    let scene = config.scenes.remove(0);
    assert_eq!(scene.name, "main");
    assert!(!scene.kill_on_exit);
    scene.chain
}

/// Wrap a bare chain as the implicit "main" scene, the way exact-parse
/// assertions expect it.
fn main_scene(chain: Vec<EffectSpec>) -> Vec<SceneSpec> {
    vec![SceneSpec {
        name: "main".to_owned(),
        kill_on_exit: false,
        chain,
    }]
}

#[test]
fn scrambled_example_parses_exactly() {
    let config = parse(include_str!("../../../examples/scrambled.kdl"));
    assert_eq!(
        config,
        Config {
            input: Some("Roland".to_owned()),
            hide_input: false,
            output: OutputSpec::Virtual("miditool Out".to_owned()),
            tempo: 120.0,
            remote_port: None,
            scenes: main_scene(vec![
                EffectSpec::ShuffleLock {
                    seed: 42,
                    lo: 21,
                    hi: 108,
                    mode: ShuffleMode::Free,
                },
                EffectSpec::VelocityCurve {
                    gamma: 0.8,
                    floor: 1,
                    ceiling: 127,
                },
            ]),
        }
    );
}

#[test]
fn split_fork_example_parses_exactly() {
    let config = parse(include_str!("../../../examples/split-fork.kdl"));
    assert_eq!(
        config,
        Config {
            input: Some("Arturia".to_owned()),
            hide_input: false,
            output: OutputSpec::Device("IAC Driver".to_owned()),
            tempo: 120.0,
            remote_port: None,
            scenes: main_scene(vec![
                EffectSpec::OnlyChannels(vec![0]),
                EffectSpec::Fork(vec![
                    EffectSpec::Chain(vec![
                        EffectSpec::KeyRange { lo: 21, hi: 59 },
                        EffectSpec::LooseKeysGaussian {
                            seed: 7,
                            sigma: 3.5
                        },
                        EffectSpec::Channelize { ch: 1 },
                    ]),
                    EffectSpec::Chain(vec![
                        EffectSpec::KeyRange { lo: 60, hi: 108 },
                        EffectSpec::Fork(vec![
                            EffectSpec::Pass,
                            EffectSpec::Transpose { semis: 12 },
                        ]),
                        EffectSpec::VelocityCurve {
                            gamma: 1.4,
                            floor: 1,
                            ceiling: 100,
                        },
                        EffectSpec::Channelize { ch: 2 },
                    ]),
                    EffectSpec::Chain(vec![
                        EffectSpec::NotesOnly,
                        EffectSpec::VelocityRange { lo: 100, hi: 127 },
                        EffectSpec::LooseKeysUniform {
                            seed: 11,
                            lo: 72,
                            hi: 96,
                        },
                        EffectSpec::Channelize { ch: 3 },
                    ]),
                ]),
            ]),
        }
    );
}

#[test]
fn echoes_example_parses_exactly() {
    let config = parse(include_str!("../../../examples/echoes.kdl"));
    assert_eq!(
        config,
        Config {
            input: Some("Roland".to_owned()),
            hide_input: false,
            output: OutputSpec::Virtual("miditool Echoes".to_owned()),
            tempo: 96.0,
            remote_port: Some(8320),
            scenes: vec![
                SceneSpec {
                    name: "echoes".to_owned(),
                    kill_on_exit: false,
                    chain: vec![
                        EffectSpec::Echo {
                            repeats: 4,
                            time: TimeSpec::Beats(0.5),
                            decay: 0.7,
                            transpose: 0,
                        },
                        EffectSpec::Restrike {
                            seed: 9,
                            interval: TimeSpec::Millis(2000.0),
                            jitter: 0.2,
                            decay: 0.7,
                            floor: 8,
                            max: 12,
                        },
                    ],
                },
                SceneSpec {
                    name: "echo storm".to_owned(),
                    kill_on_exit: true,
                    chain: vec![
                        EffectSpec::Echo {
                            repeats: 6,
                            time: TimeSpec::Millis(300.0),
                            decay: 0.8,
                            transpose: 0,
                        },
                        EffectSpec::Restrike {
                            seed: 9,
                            interval: TimeSpec::Millis(1500.0),
                            jitter: 0.3,
                            decay: 0.6,
                            floor: 8,
                            max: 12,
                        },
                    ],
                },
            ],
        }
    );
}

#[test]
fn missing_output_defaults_to_virtual_port() {
    let config = parse("pass");
    assert_eq!(
        config.output,
        OutputSpec::Virtual("miditool Out".to_owned())
    );
    assert_eq!(config.input, None);
}

#[test]
fn empty_document_is_a_valid_config() {
    let config = parse("");
    assert_eq!(
        config,
        Config {
            input: None,
            hide_input: false,
            output: OutputSpec::Virtual("miditool Out".to_owned()),
            tempo: 120.0,
            remote_port: None,
            scenes: main_scene(vec![]),
        }
    );
}

#[test]
fn input_hide_property() {
    let config = parse("input \"Roland\" hide=true");
    assert_eq!(config.input.as_deref(), Some("Roland"));
    assert!(config.hide_input);
}

#[test]
fn scene_blocks_parse_exactly() {
    let config = parse(
        "scene \"scrambled\" {\n\
             shuffle-lock seed=42\n\
             velocity-curve gamma=0.8\n\
         }\n\
         scene \"echo storm\" switch=\"kill\" {\n\
             echo repeats=6 time=\"300ms\" decay=0.8\n\
         }",
    );
    assert_eq!(
        config.scenes,
        vec![
            SceneSpec {
                name: "scrambled".to_owned(),
                kill_on_exit: false,
                chain: vec![
                    EffectSpec::ShuffleLock {
                        seed: 42,
                        lo: 21,
                        hi: 108,
                        mode: ShuffleMode::Free,
                    },
                    EffectSpec::VelocityCurve {
                        gamma: 0.8,
                        floor: 1,
                        ceiling: 127,
                    },
                ],
            },
            SceneSpec {
                name: "echo storm".to_owned(),
                kill_on_exit: true,
                chain: vec![EffectSpec::Echo {
                    repeats: 6,
                    time: TimeSpec::Millis(300.0),
                    decay: 0.8,
                    transpose: 0,
                }],
            },
        ]
    );
}

#[test]
fn scene_switch_let_ring_is_the_spelled_out_default() {
    let config = parse("scene \"a\" switch=\"let-ring\" { pass; }");
    assert!(!config.scenes[0].kill_on_exit);
}

#[test]
fn bad_switch_value_is_rejected() {
    let msg = parse_err("scene \"a\" switch=\"sustain\" { pass; }");
    assert!(
        msg.contains("sustain") && msg.contains("kill") && msg.contains("let-ring"),
        "error should show the bad value and the alternatives: {msg}"
    );
}

#[test]
fn multi_word_scene_names_parse() {
    let config = parse("scene \"late night dub\" { echo time=\"300ms\"; }");
    assert_eq!(config.scenes[0].name, "late night dub");
}

#[test]
fn duplicate_scene_name_is_rejected() {
    let msg = parse_err("scene \"a\" { pass; }\nscene \"a\" { discard; }");
    assert!(
        msg.contains("duplicate") && msg.contains("\"a\""),
        "error should name the repeated scene: {msg}"
    );
}

#[test]
fn scene_names_are_case_sensitive() {
    let config = parse("scene \"Solo\" { pass; }\nscene \"solo\" { pass; }");
    assert_eq!(config.scenes.len(), 2);
}

#[test]
fn empty_scene_name_is_rejected() {
    let msg = parse_err("scene \"\" { pass; }");
    assert!(
        msg.contains("scene") && msg.contains("empty"),
        "error should state the constraint: {msg}"
    );
}

#[test]
fn empty_scene_is_rejected() {
    let msg = parse_err("scene \"quiet\"");
    assert!(
        msg.contains("\"quiet\"") && msg.contains("effect"),
        "error should name the scene and ask for an effect: {msg}"
    );
    let msg = parse_err("scene \"quiet\" {\n}");
    assert!(msg.contains("\"quiet\""), "empty block: {msg}");
}

#[test]
fn mixing_bare_effects_and_scenes_is_rejected() {
    let msg = parse_err("pass\nscene \"a\" { discard; }");
    assert!(
        msg.contains("put the loose effects in a scene block"),
        "error should tell the fix: {msg}"
    );
    // The same in the other order.
    let msg = parse_err("scene \"a\" { discard; }\npass");
    assert!(
        msg.contains("put the loose effects in a scene block"),
        "order should not matter: {msg}"
    );
}

#[test]
fn remote_node_parses() {
    let config = parse("remote port=8320\npass");
    assert_eq!(config.remote_port, Some(8320));
}

#[test]
fn remote_defaults_to_off() {
    assert_eq!(parse("pass").remote_port, None);
}

#[test]
fn remote_requires_a_port() {
    let msg = parse_err("remote");
    assert!(
        msg.contains("port"),
        "error should name the missing property: {msg}"
    );
}

#[test]
fn remote_port_out_of_range_is_rejected() {
    let msg = parse_err("remote port=0");
    assert!(
        msg.contains("remote") && msg.contains("1..=65535") && msg.contains('0'),
        "error should name the node, the range, and the value: {msg}"
    );
    let msg = parse_err("remote port=65536");
    assert!(msg.contains("65536"), "upper bound: {msg}");
}

#[test]
fn duplicate_remote_is_rejected() {
    let msg = parse_err("remote port=8320\nremote port=8321");
    assert!(msg.contains("remote"), "error should name the node: {msg}");
}

#[test]
fn velocity_curve_defaults() {
    assert_eq!(
        parse_chain("velocity-curve"),
        vec![EffectSpec::VelocityCurve {
            gamma: 1.0,
            floor: 1,
            ceiling: 127,
        }]
    );
}

#[test]
fn shuffle_lock_defaults() {
    assert_eq!(
        parse_chain("shuffle-lock seed=1"),
        vec![EffectSpec::ShuffleLock {
            seed: 1,
            lo: 21,
            hi: 108,
            mode: ShuffleMode::Free,
        }]
    );
}

#[test]
fn shuffle_lock_modes() {
    let chain = parse_chain(
        "shuffle-lock seed=1 mode=\"within-octave\"\n\
         shuffle-lock seed=2 mode=\"within-pitch-class\"",
    );
    assert_eq!(
        chain,
        vec![
            EffectSpec::ShuffleLock {
                seed: 1,
                lo: 21,
                hi: 108,
                mode: ShuffleMode::WithinOctave,
            },
            EffectSpec::ShuffleLock {
                seed: 2,
                lo: 21,
                hi: 108,
                mode: ShuffleMode::WithinPitchClass,
            },
        ]
    );
}

#[test]
fn loose_keys_sigma_wins_over_range() {
    assert_eq!(
        parse_chain("loose-keys seed=3 lo=30 hi=90 sigma=7.0"),
        vec![EffectSpec::LooseKeysGaussian {
            seed: 3,
            sigma: 7.0,
        }]
    );
}

#[test]
fn loose_keys_defaults_to_piano_range() {
    assert_eq!(
        parse_chain("loose-keys seed=3"),
        vec![EffectSpec::LooseKeysUniform {
            seed: 3,
            lo: 21,
            hi: 108,
        }]
    );
}

#[test]
fn channels_are_rebased_sorted_and_deduplicated() {
    assert_eq!(
        parse_chain("only-channels 3 1 16 3"),
        vec![EffectSpec::OnlyChannels(vec![0, 2, 15])]
    );
}

#[test]
fn negative_transpose() {
    assert_eq!(
        parse_chain("transpose -12"),
        vec![EffectSpec::Transpose { semis: -12 }]
    );
}

#[test]
fn fork_of_chains_of_filters_round_trips() {
    let chain = parse_chain(
        "fork {\n\
             chain {\n\
                 key-range lo=0 hi=59\n\
                 notes-only\n\
                 discard\n\
             }\n\
             chain {\n\
                 velocity-range lo=64 hi=127\n\
                 controllers-only\n\
             }\n\
             pass\n\
         }",
    );
    assert_eq!(
        chain,
        vec![EffectSpec::Fork(vec![
            EffectSpec::Chain(vec![
                EffectSpec::KeyRange { lo: 0, hi: 59 },
                EffectSpec::NotesOnly,
                EffectSpec::Discard,
            ]),
            EffectSpec::Chain(vec![
                EffectSpec::VelocityRange { lo: 64, hi: 127 },
                EffectSpec::ControllersOnly,
            ]),
            EffectSpec::Pass,
        ])]
    );
}

#[test]
fn unknown_effect_node_is_reported_by_name() {
    let msg = parse_err("reverse-polarity 12");
    assert!(
        msg.contains("reverse-polarity"),
        "error should name the unknown node: {msg}"
    );
}

#[test]
fn channelize_out_of_range_is_rejected() {
    let msg = parse_err("channelize 17");
    assert!(
        msg.contains("channelize") && msg.contains("17"),
        "error should name the node and the value: {msg}"
    );
    assert!(
        msg.contains("1..=16"),
        "error should state the valid range: {msg}"
    );
}

#[test]
fn channelize_zero_is_rejected() {
    let msg = parse_err("channelize 0");
    assert!(msg.contains("1..=16"), "channels are 1-based: {msg}");
}

#[test]
fn key_range_lo_above_hi_is_rejected() {
    let msg = parse_err("key-range lo=61 hi=60");
    assert!(
        msg.contains("key-range") && msg.contains("lo=61"),
        "error should name the node and the bound: {msg}"
    );
}

#[test]
fn shuffle_lock_requires_a_seed() {
    let msg = parse_err("shuffle-lock lo=21 hi=108");
    assert!(
        msg.contains("seed"),
        "error should name the missing property: {msg}"
    );
}

#[test]
fn loose_keys_requires_a_seed() {
    let msg = parse_err("loose-keys sigma=2.0");
    assert!(
        msg.contains("seed"),
        "error should name the missing property: {msg}"
    );
}

#[test]
fn bad_shuffle_mode_is_rejected() {
    let msg = parse_err("shuffle-lock seed=1 mode=\"sideways\"");
    assert!(
        msg.contains("sideways") && msg.contains("within-octave"),
        "error should show the bad mode and the alternatives: {msg}"
    );
}

#[test]
fn gamma_must_be_positive() {
    let msg = parse_err("velocity-curve gamma=0.0");
    assert!(
        msg.contains("gamma") && msg.contains("greater than 0"),
        "error should state the constraint: {msg}"
    );
}

#[test]
fn output_requires_exactly_one_property() {
    let msg = parse_err("output");
    assert!(msg.contains("output"), "error should name the node: {msg}");

    let msg = parse_err("output virtual=\"A\" device=\"B\"");
    assert!(
        msg.contains("mutually exclusive"),
        "error should state the conflict: {msg}"
    );
}

#[test]
fn duplicate_input_is_rejected() {
    let msg = parse_err("input \"A\"\ninput \"B\"");
    assert!(msg.contains("input"), "error should name the node: {msg}");
}

#[test]
fn tempo_defaults_to_120() {
    assert_eq!(parse("pass").tempo, 120.0);
}

#[test]
fn tempo_node_accepts_integers_and_decimals() {
    assert_eq!(parse("tempo 96").tempo, 96.0);
    assert_eq!(parse("tempo 93.5").tempo, 93.5);
}

#[test]
fn tempo_out_of_range_is_rejected() {
    let msg = parse_err("tempo 10");
    assert!(
        msg.contains("tempo") && msg.contains("20..=400") && msg.contains("10"),
        "error should name the node, the range, and the value: {msg}"
    );
    let msg = parse_err("tempo 500");
    assert!(msg.contains("20..=400"), "upper bound: {msg}");
}

#[test]
fn duration_strings_in_ms_and_s() {
    let chain = parse_chain(
        "delay time=\"250ms\"\n\
         delay time=\"1.5s\"\n\
         delay time=\"2s\"\n\
         delay time=\"0.5ms\"",
    );
    assert_eq!(
        chain,
        vec![
            EffectSpec::Delay {
                time: TimeSpec::Millis(250.0)
            },
            EffectSpec::Delay {
                time: TimeSpec::Millis(1500.0)
            },
            EffectSpec::Delay {
                time: TimeSpec::Millis(2000.0)
            },
            EffectSpec::Delay {
                time: TimeSpec::Millis(0.5)
            },
        ]
    );
}

#[test]
fn delay_accepts_beats() {
    assert_eq!(
        parse_chain("delay beats=0.5"),
        vec![EffectSpec::Delay {
            time: TimeSpec::Beats(0.5)
        }]
    );
}

#[test]
fn beats_accept_integers() {
    assert_eq!(
        parse_chain("delay beats=1"),
        vec![EffectSpec::Delay {
            time: TimeSpec::Beats(1.0)
        }]
    );
}

#[test]
fn bad_duration_suffixes_are_rejected() {
    let msg = parse_err("delay time=\"250us\"");
    assert!(
        msg.contains("delay") && msg.contains("250us") && msg.contains("250ms"),
        "error should name the node, the value, and an example: {msg}"
    );
    let msg = parse_err("delay time=\"250\"");
    assert!(msg.contains("250"), "missing suffix: {msg}");
    let msg = parse_err("delay time=\"fastms\"");
    assert!(msg.contains("fastms"), "non-numeric: {msg}");
    let msg = parse_err("delay time=\"1.2.3s\"");
    assert!(msg.contains("1.2.3s"), "malformed decimal: {msg}");
    let msg = parse_err("delay time=\"1e3ms\"");
    assert!(msg.contains("1e3ms"), "exponents are not durations: {msg}");
}

#[test]
fn zero_duration_is_rejected() {
    let msg = parse_err("delay time=\"0ms\"");
    assert!(
        msg.contains("positive") && msg.contains("0ms"),
        "error should state the constraint: {msg}"
    );
}

#[test]
fn negative_beats_are_rejected() {
    let msg = parse_err("delay beats=-1.0");
    assert!(
        msg.contains("beats") && msg.contains("greater than 0"),
        "error should state the constraint: {msg}"
    );
}

#[test]
fn time_and_beats_are_mutually_exclusive() {
    let msg = parse_err("delay time=\"1s\" beats=1.0");
    assert!(
        msg.contains("mutually exclusive"),
        "error should state the conflict: {msg}"
    );
    let msg = parse_err("restrike seed=1 interval=\"1s\" beats=1.0");
    assert!(
        msg.contains("interval") && msg.contains("mutually exclusive"),
        "error should use the node's property name: {msg}"
    );
}

#[test]
fn delay_requires_a_time() {
    let msg = parse_err("delay");
    assert!(
        msg.contains("time") && msg.contains("beats"),
        "error should offer both forms: {msg}"
    );
}

#[test]
fn echo_defaults() {
    assert_eq!(
        parse_chain("echo time=\"300ms\""),
        vec![EffectSpec::Echo {
            repeats: 3,
            time: TimeSpec::Millis(300.0),
            decay: 0.6,
            transpose: 0,
        }]
    );
}

#[test]
fn echo_full_form() {
    assert_eq!(
        parse_chain("echo repeats=4 time=\"300ms\" decay=0.7 transpose=-12"),
        vec![EffectSpec::Echo {
            repeats: 4,
            time: TimeSpec::Millis(300.0),
            decay: 0.7,
            transpose: -12,
        }]
    );
}

#[test]
fn echo_decay_of_one_is_allowed() {
    let chain = parse_chain("echo time=\"300ms\" decay=1.0");
    assert!(matches!(
        chain[0],
        EffectSpec::Echo { decay, .. } if decay == 1.0
    ));
}

#[test]
fn echo_range_errors() {
    let msg = parse_err("echo repeats=0 time=\"300ms\"");
    assert!(msg.contains("echo") && msg.contains("1..=16"), "{msg}");
    let msg = parse_err("echo repeats=17 time=\"300ms\"");
    assert!(msg.contains("1..=16") && msg.contains("17"), "{msg}");
    let msg = parse_err("echo time=\"300ms\" decay=0.0");
    assert!(
        msg.contains("decay") && msg.contains("greater than 0"),
        "{msg}"
    );
    let msg = parse_err("echo time=\"300ms\" decay=1.5");
    assert!(msg.contains("at most 1"), "{msg}");
    let msg = parse_err("echo time=\"300ms\" transpose=25");
    assert!(msg.contains("-24..=24") && msg.contains("25"), "{msg}");
}

#[test]
fn restrike_defaults() {
    assert_eq!(
        parse_chain("restrike seed=1 interval=\"2s\""),
        vec![EffectSpec::Restrike {
            seed: 1,
            interval: TimeSpec::Millis(2000.0),
            jitter: 0.15,
            decay: 0.7,
            floor: 8,
            max: 12,
        }]
    );
}

#[test]
fn restrike_full_form_with_beats() {
    assert_eq!(
        parse_chain("restrike seed=1 beats=2.0 jitter=0.2 decay=0.75 floor=10 max=4"),
        vec![EffectSpec::Restrike {
            seed: 1,
            interval: TimeSpec::Beats(2.0),
            jitter: 0.2,
            decay: 0.75,
            floor: 10,
            max: 4,
        }]
    );
}

#[test]
fn restrike_requires_a_seed() {
    let msg = parse_err("restrike interval=\"2s\"");
    assert!(
        msg.contains("seed"),
        "error should name the missing property: {msg}"
    );
}

#[test]
fn restrike_range_errors() {
    let msg = parse_err("restrike seed=1 interval=\"2s\" jitter=1.0");
    assert!(msg.contains("jitter") && msg.contains("0..=0.9"), "{msg}");
    let msg = parse_err("restrike seed=1 interval=\"2s\" decay=1.0");
    assert!(
        msg.contains("decay") && msg.contains("less than 1"),
        "{msg}"
    );
    let msg = parse_err("restrike seed=1 interval=\"2s\" floor=0");
    assert!(msg.contains("floor") && msg.contains("1..=127"), "{msg}");
    let msg = parse_err("restrike seed=1 interval=\"2s\" max=25");
    assert!(msg.contains("max") && msg.contains("1..=24"), "{msg}");
}

#[test]
fn stutter_defaults() {
    assert_eq!(
        parse_chain("stutter first=\"30ms\""),
        vec![EffectSpec::Stutter {
            repeats: 6,
            first: TimeSpec::Millis(30.0),
            curve: 1.0,
        }]
    );
}

#[test]
fn stutter_full_form() {
    assert_eq!(
        parse_chain("stutter repeats=8 first=\"30ms\" curve=1.4"),
        vec![EffectSpec::Stutter {
            repeats: 8,
            first: TimeSpec::Millis(30.0),
            curve: 1.4,
        }]
    );
}

#[test]
fn stutter_range_errors() {
    let msg = parse_err("stutter repeats=25 first=\"30ms\"");
    assert!(msg.contains("stutter") && msg.contains("1..=24"), "{msg}");
    let msg = parse_err("stutter first=\"30ms\" curve=0.1");
    assert!(msg.contains("curve") && msg.contains("0.25..=4.0"), "{msg}");
    let msg = parse_err("stutter first=\"30ms\" curve=5.0");
    assert!(msg.contains("0.25..=4.0") && msg.contains("5"), "{msg}");
}

#[test]
fn to_nanos_resolves_millis_independently_of_tempo() {
    assert_eq!(TimeSpec::Millis(250.0).to_nanos(120.0), 250_000_000);
    assert_eq!(TimeSpec::Millis(250.0).to_nanos(33.3), 250_000_000);
    assert_eq!(TimeSpec::Millis(1500.0).to_nanos(120.0), 1_500_000_000);
    assert_eq!(TimeSpec::Millis(0.5).to_nanos(120.0), 500_000);
}

#[test]
fn to_nanos_resolves_beats_against_the_tempo() {
    assert_eq!(TimeSpec::Beats(1.0).to_nanos(120.0), 500_000_000);
    assert_eq!(TimeSpec::Beats(2.0).to_nanos(60.0), 2_000_000_000);
    // Half a beat at 90 bpm is a third of a second, rounded to the
    // nearest nanosecond.
    assert_eq!(TimeSpec::Beats(0.5).to_nanos(90.0), 333_333_333);
}
