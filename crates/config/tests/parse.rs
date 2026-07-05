//! End-to-end tests for the public parsing API: the shipped examples, the
//! documented defaults, and the validation errors.

use std::net::{IpAddr, Ipv4Addr};

use miditool_config::{
    ClusterAnchor, ClusterKind, Config, CrescendoShape, EffectSpec, OutputSpec, RemoteSpec,
    RowForm, SceneSpec, ShuffleMode, SieveSnap, TimeSpec, parse_str,
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
            remote: None,
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
            remote: None,
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
            remote: Some(RemoteSpec {
                port: 8320,
                bind: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            }),
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
fn scripted_example_parses_exactly() {
    let config = parse(include_str!("../../../examples/scripted.kdl"));
    assert_eq!(
        config,
        Config {
            input: Some("Roland".to_owned()),
            hide_input: false,
            output: OutputSpec::Virtual("miditool Out".to_owned()),
            tempo: 120.0,
            remote: None,
            scenes: vec![SceneSpec {
                name: "wedge".to_owned(),
                kill_on_exit: false,
                chain: vec![
                    EffectSpec::VelocityCurve {
                        gamma: 0.8,
                        floor: 1,
                        ceiling: 127,
                    },
                    EffectSpec::Script {
                        path: "wedge.lua".to_owned(),
                        seed: 1,
                    },
                ],
            }],
        }
    );
}

#[test]
fn serial_example_parses_exactly() {
    let config = parse(include_str!("../../../examples/serial.kdl"));
    assert_eq!(
        config,
        Config {
            input: Some("Roland".to_owned()),
            hide_input: false,
            output: OutputSpec::Virtual("miditool Serial".to_owned()),
            tempo: 72.0,
            remote: None,
            scenes: vec![
                SceneSpec {
                    name: "row".to_owned(),
                    kill_on_exit: false,
                    chain: vec![
                        EffectSpec::RowSnap {
                            row: [0, 11, 3, 4, 8, 7, 9, 5, 6, 1, 2, 10],
                            form: RowForm::Inversion,
                            transpose: 7,
                        },
                        EffectSpec::VelocityCurve {
                            gamma: 0.8,
                            floor: 1,
                            ceiling: 127,
                        },
                    ],
                },
                SceneSpec {
                    name: "sieve clouds".to_owned(),
                    kill_on_exit: false,
                    chain: vec![
                        EffectSpec::Sieve {
                            expr: "8@0|8@3|11@5".to_owned(),
                            snap: SieveSnap::Up,
                        },
                        EffectSpec::RegistralScatter {
                            seed: 5,
                            lo: 36,
                            hi: 96,
                        },
                        EffectSpec::Echo {
                            repeats: 3,
                            time: TimeSpec::Beats(1.0),
                            decay: 0.6,
                            transpose: 7,
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
            remote: None,
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
    assert_eq!(
        config.remote,
        Some(RemoteSpec {
            port: 8320,
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
        })
    );
}

#[test]
fn remote_defaults_to_off() {
    assert_eq!(parse("pass").remote, None);
}

#[test]
fn remote_bind_defaults_to_loopback() {
    let config = parse("remote port=8320");
    assert_eq!(config.remote.unwrap().bind, IpAddr::V4(Ipv4Addr::LOCALHOST));
}

#[test]
fn remote_bind_all_interfaces() {
    let config = parse("remote port=8320 bind=\"0.0.0.0\"");
    assert_eq!(
        config.remote,
        Some(RemoteSpec {
            port: 8320,
            bind: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        })
    );
}

#[test]
fn remote_bind_accepts_a_specific_address() {
    let config = parse("remote port=8320 bind=\"192.168.1.20\"");
    assert_eq!(
        config.remote.unwrap().bind,
        IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20))
    );
}

#[test]
fn remote_bad_bind_is_rejected() {
    let msg = parse_err("remote port=8320 bind=\"the-network\"");
    assert!(
        msg.contains("remote") && msg.contains("the-network") && msg.contains("0.0.0.0"),
        "error should name the node, the value, and an example: {msg}"
    );
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

/// `depth` nested containers around a single `pass`.
fn nested(node: &str, depth: usize) -> String {
    let mut text = String::new();
    for _ in 0..depth {
        text.push_str(node);
        text.push_str(" {\n");
    }
    text.push_str("pass\n");
    for _ in 0..depth {
        text.push_str("}\n");
    }
    text
}

#[test]
fn moderate_nesting_is_fine() {
    let chain = parse_chain(&nested("chain", 10));
    assert!(matches!(chain[0], EffectSpec::Chain(_)));
}

#[test]
fn nesting_past_the_limit_is_rejected() {
    // An explicit stack size: the raw KDL parser needs more than the
    // harness's default test-thread stack at this depth in debug builds,
    // and this test is about the limit, not the harness.
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            let msg = parse_err(&nested("chain", 65));
            assert!(
                msg.contains("chain") && msg.contains("64"),
                "error should name the node and the limit: {msg}"
            );
            let msg = parse_err(&nested("fork", 65));
            assert!(
                msg.contains("fork") && msg.contains("64"),
                "the limit covers forks too: {msg}"
            );
        })
        .expect("spawn")
        .join()
        .expect("deep nesting should be rejected, not overflow");
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
fn registral_scatter_defaults_to_piano_range() {
    assert_eq!(
        parse_chain("registral-scatter seed=7"),
        vec![EffectSpec::RegistralScatter {
            seed: 7,
            lo: 21,
            hi: 108,
        }]
    );
}

#[test]
fn registral_scatter_full_form() {
    assert_eq!(
        parse_chain("registral-scatter seed=7 lo=36 hi=96"),
        vec![EffectSpec::RegistralScatter {
            seed: 7,
            lo: 36,
            hi: 96,
        }]
    );
}

#[test]
fn registral_scatter_requires_a_seed() {
    let msg = parse_err("registral-scatter lo=36 hi=96");
    assert!(
        msg.contains("seed"),
        "error should name the missing property: {msg}"
    );
}

#[test]
fn registral_scatter_range_errors() {
    let msg = parse_err("registral-scatter seed=1 hi=128");
    assert!(
        msg.contains("registral-scatter") && msg.contains("0..=127") && msg.contains("128"),
        "{msg}"
    );
    let msg = parse_err("registral-scatter seed=1 lo=61 hi=60");
    assert!(msg.contains("lo=61"), "lo must not exceed hi: {msg}");
}

#[test]
fn wedge_mirror_defaults() {
    assert_eq!(
        parse_chain("wedge-mirror"),
        vec![EffectSpec::WedgeMirror {
            axis: 60,
            probability: 1.0,
            seed: 0,
        }]
    );
}

#[test]
fn wedge_mirror_full_form() {
    assert_eq!(
        parse_chain("wedge-mirror axis=48 probability=0.5 seed=9"),
        vec![EffectSpec::WedgeMirror {
            axis: 48,
            probability: 0.5,
            seed: 9,
        }]
    );
}

#[test]
fn wedge_mirror_range_errors() {
    let msg = parse_err("wedge-mirror axis=128");
    assert!(
        msg.contains("wedge-mirror") && msg.contains("axis") && msg.contains("0..=127"),
        "{msg}"
    );
    let msg = parse_err("wedge-mirror probability=1.5");
    assert!(
        msg.contains("probability") && msg.contains("0..=1"),
        "{msg}"
    );
    let msg = parse_err("wedge-mirror probability=-0.5");
    assert!(msg.contains("0..=1"), "lower bound: {msg}");
}

#[test]
fn blocked_keys_are_sorted_and_deduplicated() {
    assert_eq!(
        parse_chain("blocked-keys 67 60 64 60"),
        vec![EffectSpec::BlockedKeys {
            keys: vec![60, 64, 67],
            by_class: false,
        }]
    );
}

#[test]
fn blocked_keys_by_class() {
    assert_eq!(
        parse_chain("blocked-keys 7 0 4 by-class=true"),
        vec![EffectSpec::BlockedKeys {
            keys: vec![0, 4, 7],
            by_class: true,
        }]
    );
}

#[test]
fn blocked_keys_require_at_least_one_key() {
    let msg = parse_err("blocked-keys");
    assert!(
        msg.contains("blocked-keys") && msg.contains("at least one"),
        "{msg}"
    );
}

#[test]
fn blocked_keys_range_errors() {
    let msg = parse_err("blocked-keys 128");
    assert!(
        msg.contains("blocked-keys") && msg.contains("0..=127") && msg.contains("128"),
        "{msg}"
    );
    let msg = parse_err("blocked-keys 12 by-class=true");
    assert!(
        msg.contains("0..=11") && msg.contains("12"),
        "by-class narrows the range: {msg}"
    );
}

#[test]
fn klangfarben_defaults_to_cycling_in_written_order() {
    assert_eq!(
        parse_chain("klangfarben 4 2 3"),
        vec![EffectSpec::Klangfarben {
            channels: vec![3, 1, 2],
            random: false,
            seed: 0,
        }]
    );
}

#[test]
fn klangfarben_random_mode() {
    assert_eq!(
        parse_chain("klangfarben 16 1 mode=\"random\" seed=3"),
        vec![EffectSpec::Klangfarben {
            channels: vec![15, 0],
            random: true,
            seed: 3,
        }]
    );
}

#[test]
fn klangfarben_requires_at_least_one_channel() {
    let msg = parse_err("klangfarben");
    assert!(
        msg.contains("klangfarben") && msg.contains("at least one"),
        "{msg}"
    );
}

#[test]
fn klangfarben_rejects_out_of_range_channels() {
    let msg = parse_err("klangfarben 0");
    assert!(msg.contains("1..=16"), "channels are 1-based: {msg}");
    let msg = parse_err("klangfarben 17");
    assert!(msg.contains("1..=16") && msg.contains("17"), "{msg}");
}

#[test]
fn klangfarben_rejects_duplicate_channels() {
    let msg = parse_err("klangfarben 2 3 2");
    assert!(
        msg.contains("klangfarben") && msg.contains("2") && msg.contains("once"),
        "error should name the repeated channel: {msg}"
    );
}

#[test]
fn klangfarben_bad_mode_is_rejected() {
    let msg = parse_err("klangfarben 2 3 mode=\"sideways\"");
    assert!(
        msg.contains("sideways") && msg.contains("cycle") && msg.contains("random"),
        "error should show the bad mode and the alternatives: {msg}"
    );
}

#[test]
fn ring_mod_defaults() {
    assert_eq!(
        parse_chain("ring-mod carrier=60"),
        vec![EffectSpec::RingMod {
            carrier: 60,
            sum: true,
            diff: true,
            dry: false,
        }]
    );
}

#[test]
fn ring_mod_full_form() {
    assert_eq!(
        parse_chain("ring-mod carrier=48 sum=false diff=true dry=true"),
        vec![EffectSpec::RingMod {
            carrier: 48,
            sum: false,
            diff: true,
            dry: true,
        }]
    );
}

#[test]
fn ring_mod_requires_a_carrier() {
    let msg = parse_err("ring-mod");
    assert!(
        msg.contains("carrier"),
        "error should name the missing property: {msg}"
    );
}

#[test]
fn ring_mod_carrier_out_of_range_is_rejected() {
    let msg = parse_err("ring-mod carrier=128");
    assert!(
        msg.contains("ring-mod") && msg.contains("0..=127") && msg.contains("128"),
        "{msg}"
    );
}

#[test]
fn ring_mod_with_every_output_off_is_rejected() {
    let msg = parse_err("ring-mod carrier=60 sum=false diff=false");
    assert!(
        msg.contains("sum") && msg.contains("diff") && msg.contains("dry"),
        "error should name the constraint: {msg}"
    );
}

#[test]
fn telescope_defaults_to_middle_c_reference() {
    assert_eq!(
        parse_chain("telescope factor=2.0"),
        vec![EffectSpec::Telescope {
            factor: 2.0,
            reference: 60,
        }]
    );
}

#[test]
fn telescope_factor_accepts_integers_and_decimals() {
    assert_eq!(
        parse_chain("telescope factor=2 reference=48"),
        vec![EffectSpec::Telescope {
            factor: 2.0,
            reference: 48,
        }]
    );
    assert_eq!(
        parse_chain("telescope factor=0.5"),
        vec![EffectSpec::Telescope {
            factor: 0.5,
            reference: 60,
        }]
    );
}

#[test]
fn telescope_requires_a_factor() {
    let msg = parse_err("telescope");
    assert!(
        msg.contains("factor"),
        "error should name the missing property: {msg}"
    );
}

#[test]
fn telescope_range_errors() {
    let msg = parse_err("telescope factor=0.05");
    assert!(
        msg.contains("telescope") && msg.contains("0.1..=8"),
        "{msg}"
    );
    let msg = parse_err("telescope factor=9.0");
    assert!(msg.contains("0.1..=8") && msg.contains("9"), "{msg}");
    let msg = parse_err("telescope factor=2.0 reference=128");
    assert!(
        msg.contains("reference") && msg.contains("0..=127"),
        "{msg}"
    );
}

#[test]
fn row_snap_defaults_to_prime() {
    assert_eq!(
        parse_chain("row-snap 0 1 2 3 4 5 6 7 8 9 10 11"),
        vec![EffectSpec::RowSnap {
            row: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            form: RowForm::Prime,
            transpose: 0,
        }]
    );
}

#[test]
fn row_snap_forms() {
    let chain = parse_chain(
        "row-snap 0 1 2 3 4 5 6 7 8 9 10 11 form=\"p\"\n\
         row-snap 0 1 2 3 4 5 6 7 8 9 10 11 form=\"i\"\n\
         row-snap 0 1 2 3 4 5 6 7 8 9 10 11 form=\"r\"\n\
         row-snap 0 1 2 3 4 5 6 7 8 9 10 11 form=\"ri\" transpose=-12",
    );
    let forms: Vec<_> = chain
        .iter()
        .map(|spec| match spec {
            EffectSpec::RowSnap {
                form, transpose, ..
            } => (*form, *transpose),
            other => panic!("expected row-snap, got {other:?}"),
        })
        .collect();
    assert_eq!(
        forms,
        vec![
            (RowForm::Prime, 0),
            (RowForm::Inversion, 0),
            (RowForm::Retrograde, 0),
            (RowForm::RetrogradeInversion, -12),
        ]
    );
}

#[test]
fn row_snap_requires_exactly_twelve_entries() {
    let msg = parse_err("row-snap 0 1 2 3 4 5 6 7 8 9 10");
    assert!(
        msg.contains("row-snap") && msg.contains("12") && msg.contains("11"),
        "error should state the required and the given count: {msg}"
    );
    let msg = parse_err("row-snap 0 1 2 3 4 5 6 7 8 9 10 11 0");
    assert!(msg.contains("12") && msg.contains("13"), "too many: {msg}");
}

#[test]
fn row_snap_rejects_entries_outside_pitch_classes() {
    let msg = parse_err("row-snap 0 1 2 3 4 5 6 7 8 9 10 12");
    assert!(
        msg.contains("row-snap") && msg.contains("0..=11") && msg.contains("12"),
        "{msg}"
    );
}

#[test]
fn row_snap_names_duplicated_and_missing_pitch_classes() {
    let msg = parse_err("row-snap 0 0 2 3 4 5 6 7 8 9 10 10");
    assert!(
        msg.contains("duplicated: 0, 10") && msg.contains("missing: 1, 11"),
        "error should name what is duplicated and what is missing: {msg}"
    );
}

#[test]
fn row_snap_bad_form_is_rejected() {
    let msg = parse_err("row-snap 0 1 2 3 4 5 6 7 8 9 10 11 form=\"prime\"");
    assert!(
        msg.contains("prime") && msg.contains("\"ri\""),
        "error should show the bad form and the alternatives: {msg}"
    );
}

#[test]
fn row_snap_transpose_out_of_range_is_rejected() {
    let msg = parse_err("row-snap 0 1 2 3 4 5 6 7 8 9 10 11 transpose=25");
    assert!(msg.contains("-24..=24") && msg.contains("25"), "{msg}");
}

#[test]
fn aggregate_gate_defaults() {
    assert_eq!(
        parse_chain("aggregate-gate"),
        vec![EffectSpec::AggregateGate { leak: 0.0, seed: 0 }]
    );
}

#[test]
fn aggregate_gate_full_form() {
    assert_eq!(
        parse_chain("aggregate-gate leak=0.3 seed=4"),
        vec![EffectSpec::AggregateGate { leak: 0.3, seed: 4 }]
    );
}

#[test]
fn aggregate_gate_leak_out_of_range_is_rejected() {
    let msg = parse_err("aggregate-gate leak=1.5");
    assert!(
        msg.contains("aggregate-gate") && msg.contains("leak") && msg.contains("0..=1"),
        "{msg}"
    );
    let msg = parse_err("aggregate-gate leak=-0.1");
    assert!(msg.contains("0..=1"), "lower bound: {msg}");
}

#[test]
fn sieve_snap_defaults_to_nearest() {
    assert_eq!(
        parse_chain("sieve \"8@0|8@3|11@5\""),
        vec![EffectSpec::Sieve {
            expr: "8@0|8@3|11@5".to_owned(),
            snap: SieveSnap::Nearest,
        }]
    );
}

#[test]
fn sieve_snap_modes() {
    let chain = parse_chain(
        "sieve \"8@0\" snap=\"nearest\"\n\
         sieve \"8@0\" snap=\"up\"\n\
         sieve \"8@0\" snap=\"down\"\n\
         sieve \"8@0\" snap=\"drop\"",
    );
    let snaps: Vec<_> = chain
        .iter()
        .map(|spec| match spec {
            EffectSpec::Sieve { snap, .. } => *snap,
            other => panic!("expected sieve, got {other:?}"),
        })
        .collect();
    assert_eq!(
        snaps,
        vec![
            SieveSnap::Nearest,
            SieveSnap::Up,
            SieveSnap::Down,
            SieveSnap::Drop,
        ]
    );
}

#[test]
fn empty_sieve_expression_is_rejected() {
    let msg = parse_err("sieve \"\"");
    assert!(
        msg.contains("sieve") && msg.contains("empty"),
        "error should state the constraint: {msg}"
    );
}

#[test]
fn sieve_bad_snap_is_rejected() {
    let msg = parse_err("sieve \"8@0\" snap=\"sideways\"");
    assert!(
        msg.contains("sideways") && msg.contains("nearest"),
        "error should show the bad snap and the alternatives: {msg}"
    );
}

#[test]
fn script_parses_exactly() {
    assert_eq!(
        parse_chain("script \"wedge.lua\" seed=42"),
        vec![EffectSpec::Script {
            path: "wedge.lua".to_owned(),
            seed: 42,
        }]
    );
}

#[test]
fn script_seed_defaults_to_zero() {
    assert_eq!(
        parse_chain("script \"wedge.lua\""),
        vec![EffectSpec::Script {
            path: "wedge.lua".to_owned(),
            seed: 0,
        }]
    );
}

#[test]
fn empty_script_path_is_rejected() {
    let msg = parse_err("script \"\"");
    assert!(
        msg.contains("script") && msg.contains("empty"),
        "error should state the constraint: {msg}"
    );
}

#[test]
fn clouds_example_parses_exactly() {
    let config = parse(include_str!("../../../examples/clouds.kdl"));
    assert_eq!(
        config,
        Config {
            input: Some("Roland".to_owned()),
            hide_input: false,
            output: OutputSpec::Virtual("miditool Clouds".to_owned()),
            tempo: 84.0,
            remote: None,
            scenes: vec![
                SceneSpec {
                    name: "clouds".to_owned(),
                    kill_on_exit: false,
                    chain: vec![
                        EffectSpec::PoissonCloud {
                            seed: 17,
                            density: 12.0,
                            duration: TimeSpec::Millis(1500.0),
                            sigma: 9.0,
                            vel_sigma: 12.0,
                            max: 20,
                        },
                        EffectSpec::VelocityDiceUniform {
                            seed: 4,
                            lo: 30,
                            hi: 110,
                        },
                    ],
                },
                SceneSpec {
                    name: "cowell".to_owned(),
                    kill_on_exit: false,
                    chain: vec![
                        EffectSpec::ClusterFist {
                            kind: ClusterKind::White,
                            width: 6,
                            anchor: ClusterAnchor::Bottom,
                            rolloff: 0.7,
                        },
                        EffectSpec::ResonanceHalo {
                            width: 2,
                            level: 0.2,
                            decay: TimeSpec::Millis(2000.0),
                            sieve: None,
                        },
                        EffectSpec::DensityGovernor {
                            seed: 3,
                            target: 6.0,
                            window: TimeSpec::Millis(1000.0),
                        },
                    ],
                },
            ],
        }
    );
}

#[test]
fn poisson_cloud_defaults() {
    assert_eq!(
        parse_chain("poisson-cloud seed=1"),
        vec![EffectSpec::PoissonCloud {
            seed: 1,
            density: 8.0,
            duration: TimeSpec::Millis(2000.0),
            sigma: 7.0,
            vel_sigma: 10.0,
            max: 16,
        }]
    );
}

#[test]
fn poisson_cloud_full_form_with_beats() {
    assert_eq!(
        parse_chain("poisson-cloud seed=2 density=20 beats=4 sigma=3.5 vel-sigma=0.0 max=8"),
        vec![EffectSpec::PoissonCloud {
            seed: 2,
            density: 20.0,
            duration: TimeSpec::Beats(4.0),
            sigma: 3.5,
            vel_sigma: 0.0,
            max: 8,
        }]
    );
}

#[test]
fn poisson_cloud_requires_a_seed() {
    let msg = parse_err("poisson-cloud density=8.0");
    assert!(
        msg.contains("seed"),
        "error should name the missing property: {msg}"
    );
}

#[test]
fn poisson_cloud_duration_and_beats_are_mutually_exclusive() {
    let msg = parse_err("poisson-cloud seed=1 duration=\"2s\" beats=1");
    assert!(
        msg.contains("duration") && msg.contains("mutually exclusive"),
        "error should use the node's property name: {msg}"
    );
}

#[test]
fn poisson_cloud_range_errors() {
    let msg = parse_err("poisson-cloud seed=1 density=0.05");
    assert!(
        msg.contains("poisson-cloud") && msg.contains("density") && msg.contains("0.1..=50"),
        "{msg}"
    );
    let msg = parse_err("poisson-cloud seed=1 density=51");
    assert!(msg.contains("0.1..=50") && msg.contains("51"), "{msg}");
    let msg = parse_err("poisson-cloud seed=1 sigma=-0.5");
    assert!(msg.contains("sigma") && msg.contains("0..=24"), "{msg}");
    let msg = parse_err("poisson-cloud seed=1 sigma=24.5");
    assert!(msg.contains("0..=24"), "upper bound: {msg}");
    let msg = parse_err("poisson-cloud seed=1 vel-sigma=40.5");
    assert!(msg.contains("vel-sigma") && msg.contains("0..=40"), "{msg}");
    let msg = parse_err("poisson-cloud seed=1 vel-sigma=-1.0");
    assert!(msg.contains("0..=40"), "lower bound: {msg}");
    let msg = parse_err("poisson-cloud seed=1 max=0");
    assert!(msg.contains("max") && msg.contains("1..=24"), "{msg}");
    let msg = parse_err("poisson-cloud seed=1 max=25");
    assert!(msg.contains("1..=24") && msg.contains("25"), "{msg}");
}

#[test]
fn note_roulette_defaults() {
    assert_eq!(
        parse_chain("note-roulette seed=6"),
        vec![EffectSpec::NoteRoulette {
            seed: 6,
            pass: 0.6,
            replace: 0.3,
            lo: 21,
            hi: 108,
        }]
    );
}

#[test]
fn note_roulette_full_form() {
    assert_eq!(
        parse_chain("note-roulette seed=6 pass=0.5 replace=0.5 lo=36 hi=96"),
        vec![EffectSpec::NoteRoulette {
            seed: 6,
            pass: 0.5,
            replace: 0.5,
            lo: 36,
            hi: 96,
        }]
    );
}

#[test]
fn note_roulette_requires_a_seed() {
    let msg = parse_err("note-roulette pass=0.5");
    assert!(
        msg.contains("seed"),
        "error should name the missing property: {msg}"
    );
}

#[test]
fn note_roulette_pass_and_replace_must_not_sum_past_one() {
    let msg = parse_err("note-roulette seed=1 pass=0.7 replace=0.4");
    assert!(
        msg.contains("pass=0.7") && msg.contains("replace=0.4") && msg.contains("at most 1"),
        "error should name both probabilities: {msg}"
    );
}

#[test]
fn note_roulette_range_errors() {
    let msg = parse_err("note-roulette seed=1 pass=1.5");
    assert!(
        msg.contains("note-roulette") && msg.contains("pass") && msg.contains("0..=1"),
        "{msg}"
    );
    let msg = parse_err("note-roulette seed=1 replace=-0.1");
    assert!(msg.contains("replace") && msg.contains("0..=1"), "{msg}");
    let msg = parse_err("note-roulette seed=1 hi=128");
    assert!(msg.contains("0..=127") && msg.contains("128"), "{msg}");
    let msg = parse_err("note-roulette seed=1 lo=61 hi=60");
    assert!(msg.contains("lo=61"), "lo must not exceed hi: {msg}");
}

#[test]
fn velocity_dice_defaults_to_the_full_velocity_range() {
    assert_eq!(
        parse_chain("velocity-dice seed=2"),
        vec![EffectSpec::VelocityDiceUniform {
            seed: 2,
            lo: 1,
            hi: 127,
        }]
    );
}

#[test]
fn velocity_dice_sigma_wins_over_range() {
    assert_eq!(
        parse_chain("velocity-dice seed=2 lo=30 hi=110 sigma=15.0"),
        vec![EffectSpec::VelocityDiceGaussian {
            seed: 2,
            sigma: 15.0,
        }]
    );
}

#[test]
fn velocity_dice_requires_a_seed() {
    let msg = parse_err("velocity-dice sigma=15.0");
    assert!(
        msg.contains("seed"),
        "error should name the missing property: {msg}"
    );
}

#[test]
fn velocity_dice_range_errors() {
    let msg = parse_err("velocity-dice seed=1 lo=0");
    assert!(
        msg.contains("velocity-dice") && msg.contains("lo") && msg.contains("1..=127"),
        "{msg}"
    );
    let msg = parse_err("velocity-dice seed=1 hi=128");
    assert!(msg.contains("1..=127") && msg.contains("128"), "{msg}");
    let msg = parse_err("velocity-dice seed=1 lo=100 hi=50");
    assert!(msg.contains("lo=100"), "lo must not exceed hi: {msg}");
    let msg = parse_err("velocity-dice seed=1 sigma=0.05");
    assert!(msg.contains("sigma") && msg.contains("0.1..=40"), "{msg}");
    let msg = parse_err("velocity-dice seed=1 sigma=40.5");
    assert!(msg.contains("0.1..=40"), "upper bound: {msg}");
}

#[test]
fn duration_lottery_defaults() {
    assert_eq!(
        parse_chain("duration-lottery seed=8"),
        vec![EffectSpec::DurationLottery {
            seed: 8,
            mean: TimeSpec::Millis(500.0),
            min: TimeSpec::Millis(30.0),
            max: TimeSpec::Millis(4000.0),
            uniform: false,
        }]
    );
}

#[test]
fn duration_lottery_full_form() {
    assert_eq!(
        parse_chain(
            "duration-lottery seed=8 mean=\"1s\" min=\"100ms\" max=\"2s\" spread=\"uniform\""
        ),
        vec![EffectSpec::DurationLottery {
            seed: 8,
            mean: TimeSpec::Millis(1000.0),
            min: TimeSpec::Millis(100.0),
            max: TimeSpec::Millis(2000.0),
            uniform: true,
        }]
    );
}

#[test]
fn duration_lottery_mean_accepts_beats() {
    // Only the mean follows the duration-or-beats convention; min= and
    // max= stay plain duration strings.
    assert_eq!(
        parse_chain("duration-lottery seed=8 beats=0.5"),
        vec![EffectSpec::DurationLottery {
            seed: 8,
            mean: TimeSpec::Beats(0.5),
            min: TimeSpec::Millis(30.0),
            max: TimeSpec::Millis(4000.0),
            uniform: false,
        }]
    );
}

#[test]
fn duration_lottery_requires_a_seed() {
    let msg = parse_err("duration-lottery mean=\"1s\"");
    assert!(
        msg.contains("seed"),
        "error should name the missing property: {msg}"
    );
}

#[test]
fn duration_lottery_ordering_errors() {
    let msg = parse_err("duration-lottery seed=1 mean=\"500ms\" min=\"600ms\"");
    assert!(
        msg.contains("duration-lottery") && msg.contains("min=600ms") && msg.contains("mean=500ms"),
        "min must not exceed mean: {msg}"
    );
    let msg = parse_err("duration-lottery seed=1 mean=\"5s\" max=\"4s\"");
    assert!(
        msg.contains("mean=5000ms") && msg.contains("max=4000ms"),
        "mean must not exceed max: {msg}"
    );
    // With the mean in beats the mean check waits for the tempo, but a
    // min above the max is wrong regardless.
    let msg = parse_err("duration-lottery seed=1 beats=1 min=\"5s\" max=\"1s\"");
    assert!(
        msg.contains("min=5000ms") && msg.contains("max=1000ms"),
        "min must not exceed max: {msg}"
    );
}

#[test]
fn duration_lottery_bad_spread_is_rejected() {
    let msg = parse_err("duration-lottery seed=1 spread=\"gauss\"");
    assert!(
        msg.contains("gauss") && msg.contains("exp") && msg.contains("uniform"),
        "error should show the bad spread and the alternatives: {msg}"
    );
}

#[test]
fn density_governor_defaults() {
    assert_eq!(
        parse_chain("density-governor target=6"),
        vec![EffectSpec::DensityGovernor {
            seed: 0,
            target: 6.0,
            window: TimeSpec::Millis(2000.0),
        }]
    );
}

#[test]
fn density_governor_full_form_with_beats() {
    assert_eq!(
        parse_chain("density-governor target=2.5 beats=4 seed=9"),
        vec![EffectSpec::DensityGovernor {
            seed: 9,
            target: 2.5,
            window: TimeSpec::Beats(4.0),
        }]
    );
}

#[test]
fn density_governor_requires_a_target() {
    let msg = parse_err("density-governor");
    assert!(
        msg.contains("target"),
        "error should name the missing property: {msg}"
    );
}

#[test]
fn density_governor_target_range_errors() {
    let msg = parse_err("density-governor target=0.05");
    assert!(
        msg.contains("density-governor") && msg.contains("target") && msg.contains("0.1..=100"),
        "{msg}"
    );
    let msg = parse_err("density-governor target=101");
    assert!(msg.contains("0.1..=100") && msg.contains("101"), "{msg}");
}

#[test]
fn cluster_fist_defaults() {
    assert_eq!(
        parse_chain("cluster-fist"),
        vec![EffectSpec::ClusterFist {
            kind: ClusterKind::Chromatic,
            width: 4,
            anchor: ClusterAnchor::Center,
            rolloff: 0.8,
        }]
    );
}

#[test]
fn cluster_fist_kinds_and_anchors() {
    let chain = parse_chain(
        "cluster-fist kind=\"white\" anchor=\"bottom\"\n\
         cluster-fist kind=\"black\" anchor=\"top\"\n\
         cluster-fist kind=\"sieve\" sieve=\"8@0|8@3\"",
    );
    assert_eq!(
        chain,
        vec![
            EffectSpec::ClusterFist {
                kind: ClusterKind::White,
                width: 4,
                anchor: ClusterAnchor::Bottom,
                rolloff: 0.8,
            },
            EffectSpec::ClusterFist {
                kind: ClusterKind::Black,
                width: 4,
                anchor: ClusterAnchor::Top,
                rolloff: 0.8,
            },
            EffectSpec::ClusterFist {
                kind: ClusterKind::Sieve("8@0|8@3".to_owned()),
                width: 4,
                anchor: ClusterAnchor::Center,
                rolloff: 0.8,
            },
        ]
    );
}

#[test]
fn cluster_fist_sieve_kind_requires_an_expression() {
    let msg = parse_err("cluster-fist kind=\"sieve\"");
    assert!(
        msg.contains("cluster-fist") && msg.contains("kind=\"sieve\"") && msg.contains("sieve="),
        "error should ask for the expression: {msg}"
    );
}

#[test]
fn cluster_fist_sieve_without_sieve_kind_is_rejected() {
    let msg = parse_err("cluster-fist kind=\"white\" sieve=\"8@0\"");
    assert!(
        msg.contains("cluster-fist") && msg.contains("kind=\"sieve\""),
        "error should say what sieve= needs: {msg}"
    );
    let msg = parse_err("cluster-fist sieve=\"8@0\"");
    assert!(
        msg.contains("kind=\"sieve\""),
        "the default kind is chromatic: {msg}"
    );
}

#[test]
fn cluster_fist_empty_sieve_expression_is_rejected() {
    let msg = parse_err("cluster-fist kind=\"sieve\" sieve=\"\"");
    assert!(
        msg.contains("cluster-fist") && msg.contains("empty"),
        "error should state the constraint: {msg}"
    );
}

#[test]
fn cluster_fist_range_errors() {
    let msg = parse_err("cluster-fist width=1");
    assert!(
        msg.contains("cluster-fist") && msg.contains("width") && msg.contains("2..=12"),
        "{msg}"
    );
    let msg = parse_err("cluster-fist width=13");
    assert!(msg.contains("2..=12") && msg.contains("13"), "{msg}");
    let msg = parse_err("cluster-fist rolloff=1.5");
    assert!(msg.contains("rolloff") && msg.contains("0..=1"), "{msg}");
    let msg = parse_err("cluster-fist rolloff=-0.1");
    assert!(msg.contains("0..=1"), "lower bound: {msg}");
}

#[test]
fn cluster_fist_bad_kind_and_anchor_are_rejected() {
    let msg = parse_err("cluster-fist kind=\"forearm\"");
    assert!(
        msg.contains("forearm") && msg.contains("chromatic") && msg.contains("sieve"),
        "error should show the bad kind and the alternatives: {msg}"
    );
    let msg = parse_err("cluster-fist anchor=\"middle\"");
    assert!(
        msg.contains("middle") && msg.contains("center") && msg.contains("bottom"),
        "error should show the bad anchor and the alternatives: {msg}"
    );
}

#[test]
fn resonance_halo_defaults() {
    assert_eq!(
        parse_chain("resonance-halo"),
        vec![EffectSpec::ResonanceHalo {
            width: 3,
            level: 0.25,
            decay: TimeSpec::Millis(3000.0),
            sieve: None,
        }]
    );
}

#[test]
fn resonance_halo_full_form_with_beats() {
    assert_eq!(
        parse_chain("resonance-halo width=1 level=0.5 beats=2 sieve=\"8@0|8@3\""),
        vec![EffectSpec::ResonanceHalo {
            width: 1,
            level: 0.5,
            decay: TimeSpec::Beats(2.0),
            sieve: Some("8@0|8@3".to_owned()),
        }]
    );
}

#[test]
fn resonance_halo_empty_sieve_expression_is_rejected() {
    let msg = parse_err("resonance-halo sieve=\"\"");
    assert!(
        msg.contains("resonance-halo") && msg.contains("empty"),
        "error should state the constraint: {msg}"
    );
}

#[test]
fn resonance_halo_range_errors() {
    let msg = parse_err("resonance-halo width=0");
    assert!(
        msg.contains("resonance-halo") && msg.contains("width") && msg.contains("1..=6"),
        "{msg}"
    );
    let msg = parse_err("resonance-halo width=7");
    assert!(msg.contains("1..=6") && msg.contains("7"), "{msg}");
    let msg = parse_err("resonance-halo level=1.5");
    assert!(msg.contains("level") && msg.contains("0..=1"), "{msg}");
    let msg = parse_err("resonance-halo level=-0.1");
    assert!(msg.contains("0..=1"), "lower bound: {msg}");
}

#[test]
fn rhythm_example_parses_exactly() {
    let config = parse(include_str!("../../../examples/rhythm.kdl"));
    assert_eq!(
        config,
        Config {
            input: Some("Roland".to_owned()),
            hide_input: false,
            output: OutputSpec::Virtual("miditool Rhythm".to_owned()),
            tempo: 100.0,
            remote: None,
            scenes: vec![
                SceneSpec {
                    name: "tresillo".to_owned(),
                    kill_on_exit: false,
                    chain: vec![
                        EffectSpec::EuclideanGate {
                            k: 3,
                            n: 8,
                            rotation: 0,
                            pulse: TimeSpec::Beats(0.5),
                            defer: true,
                        },
                        EffectSpec::AccentGroups {
                            groups: vec![3, 3, 2],
                            accent: 118,
                            rest: 72,
                        },
                    ],
                },
                SceneSpec {
                    name: "feldman".to_owned(),
                    kill_on_exit: false,
                    chain: vec![
                        EffectSpec::FeldmanField {
                            seed: 6,
                            floor: 6,
                            ceiling: 24,
                            jitter: 3,
                        },
                        EffectSpec::AddedValue {
                            seed: 11,
                            unit: TimeSpec::Millis(80.0),
                            extend: 0.4,
                            defer: 0.0,
                        },
                        EffectSpec::AntiAccent {
                            seed: 2,
                            level: 12,
                            every: TimeSpec::Millis(30000.0),
                        },
                    ],
                },
            ],
        }
    );
}

#[test]
fn euclidean_gate_defaults_to_a_deferred_sixteenth_pulse() {
    assert_eq!(
        parse_chain("euclidean-gate k=3 n=8"),
        vec![EffectSpec::EuclideanGate {
            k: 3,
            n: 8,
            rotation: 0,
            pulse: TimeSpec::Beats(0.25),
            defer: true,
        }]
    );
}

#[test]
fn euclidean_gate_full_form() {
    assert_eq!(
        parse_chain("euclidean-gate k=5 n=13 rotation=2 pulse=\"125ms\" mode=\"drop\""),
        vec![EffectSpec::EuclideanGate {
            k: 5,
            n: 13,
            rotation: 2,
            pulse: TimeSpec::Millis(125.0),
            defer: false,
        }]
    );
}

#[test]
fn euclidean_gate_requires_k_and_n() {
    let msg = parse_err("euclidean-gate n=8");
    assert!(
        msg.contains("k"),
        "error should name the missing property: {msg}"
    );
    let msg = parse_err("euclidean-gate k=3");
    assert!(
        msg.contains("n"),
        "error should name the missing property: {msg}"
    );
}

#[test]
fn euclidean_gate_range_errors() {
    let msg = parse_err("euclidean-gate k=0 n=8");
    assert!(
        msg.contains("euclidean-gate") && msg.contains("k") && msg.contains("1..=64"),
        "{msg}"
    );
    let msg = parse_err("euclidean-gate k=3 n=65");
    assert!(msg.contains("1..=64") && msg.contains("65"), "{msg}");
    let msg = parse_err("euclidean-gate k=9 n=8");
    assert!(msg.contains("k=9"), "k must not exceed n: {msg}");
    let msg = parse_err("euclidean-gate k=3 n=8 rotation=8");
    assert!(
        msg.contains("rotation") && msg.contains("less than n=8"),
        "rotation stays below n: {msg}"
    );
}

#[test]
fn euclidean_gate_bad_mode_is_rejected() {
    let msg = parse_err("euclidean-gate k=3 n=8 mode=\"hold\"");
    assert!(
        msg.contains("hold") && msg.contains("defer") && msg.contains("drop"),
        "error should show the bad mode and the alternatives: {msg}"
    );
}

#[test]
fn euclidean_gate_pulse_and_beats_are_mutually_exclusive() {
    let msg = parse_err("euclidean-gate k=3 n=8 pulse=\"125ms\" beats=0.25");
    assert!(
        msg.contains("pulse") && msg.contains("mutually exclusive"),
        "error should use the node's property name: {msg}"
    );
}

#[test]
fn quantize_defaults_to_a_hard_sixteenth_grid() {
    assert_eq!(
        parse_chain("quantize"),
        vec![EffectSpec::Quantize {
            grid: TimeSpec::Beats(0.25),
            strength: 1.0,
        }]
    );
}

#[test]
fn quantize_full_form() {
    assert_eq!(
        parse_chain("quantize grid=\"125ms\" strength=0.5"),
        vec![EffectSpec::Quantize {
            grid: TimeSpec::Millis(125.0),
            strength: 0.5,
        }]
    );
}

#[test]
fn quantize_strength_out_of_range_is_rejected() {
    let msg = parse_err("quantize strength=1.5");
    assert!(
        msg.contains("quantize") && msg.contains("strength") && msg.contains("0..=1"),
        "{msg}"
    );
    let msg = parse_err("quantize strength=-0.1");
    assert!(msg.contains("0..=1"), "lower bound: {msg}");
}

#[test]
fn talea_entries_are_milliseconds_by_default() {
    assert_eq!(
        parse_chain("talea 250 500 250 1000"),
        vec![EffectSpec::Talea {
            durations: vec![
                TimeSpec::Millis(250.0),
                TimeSpec::Millis(500.0),
                TimeSpec::Millis(250.0),
                TimeSpec::Millis(1000.0),
            ],
        }]
    );
}

#[test]
fn talea_beats_true_reads_entries_as_beats() {
    assert_eq!(
        parse_chain("talea 1 0.5 0.5 2 beats=true"),
        vec![EffectSpec::Talea {
            durations: vec![
                TimeSpec::Beats(1.0),
                TimeSpec::Beats(0.5),
                TimeSpec::Beats(0.5),
                TimeSpec::Beats(2.0),
            ],
        }]
    );
}

#[test]
fn talea_requires_one_to_thirty_two_entries() {
    let msg = parse_err("talea");
    assert!(
        msg.contains("talea") && msg.contains("1 and 32") && msg.contains("got 0"),
        "{msg}"
    );
    let many = format!("talea{}", " 250".repeat(33));
    let msg = parse_err(&many);
    assert!(msg.contains("1 and 32") && msg.contains("33"), "{msg}");
}

#[test]
fn talea_millisecond_entries_out_of_range_name_the_offender() {
    let msg = parse_err("talea 250 0.5 500");
    assert!(
        msg.contains("talea") && msg.contains("1ms..=60s") && msg.contains("0.5ms"),
        "error should name the offending entry: {msg}"
    );
    let msg = parse_err("talea 250 61000");
    assert!(
        msg.contains("1ms..=60s") && msg.contains("61000ms"),
        "upper bound: {msg}"
    );
}

#[test]
fn talea_beat_entries_must_be_positive() {
    let msg = parse_err("talea 1 0 beats=true");
    assert!(
        msg.contains("talea") && msg.contains("greater than 0") && msg.contains("got 0"),
        "error should name the offending entry: {msg}"
    );
}

#[test]
fn added_value_defaults() {
    assert_eq!(
        parse_chain("added-value seed=5"),
        vec![EffectSpec::AddedValue {
            seed: 5,
            unit: TimeSpec::Millis(60.0),
            extend: 0.3,
            defer: 0.0,
        }]
    );
}

#[test]
fn added_value_full_form_with_beats() {
    assert_eq!(
        parse_chain("added-value seed=5 beats=0.25 extend=0.5 defer=0.2"),
        vec![EffectSpec::AddedValue {
            seed: 5,
            unit: TimeSpec::Beats(0.25),
            extend: 0.5,
            defer: 0.2,
        }]
    );
}

#[test]
fn added_value_requires_a_seed() {
    let msg = parse_err("added-value extend=0.5");
    assert!(
        msg.contains("seed"),
        "error should name the missing property: {msg}"
    );
}

#[test]
fn added_value_range_errors() {
    let msg = parse_err("added-value seed=1 extend=1.5");
    assert!(
        msg.contains("added-value") && msg.contains("extend") && msg.contains("0..=1"),
        "{msg}"
    );
    let msg = parse_err("added-value seed=1 defer=-0.1");
    assert!(msg.contains("defer") && msg.contains("0..=1"), "{msg}");
}

#[test]
fn accent_groups_defaults() {
    assert_eq!(
        parse_chain("accent-groups 3 5"),
        vec![EffectSpec::AccentGroups {
            groups: vec![3, 5],
            accent: 112,
            rest: 64,
        }]
    );
}

#[test]
fn accent_groups_full_form() {
    assert_eq!(
        parse_chain("accent-groups 2 2 3 accent=120 rest=50"),
        vec![EffectSpec::AccentGroups {
            groups: vec![2, 2, 3],
            accent: 120,
            rest: 50,
        }]
    );
}

#[test]
fn accent_groups_require_at_least_one_group() {
    let msg = parse_err("accent-groups");
    assert!(
        msg.contains("accent-groups") && msg.contains("at least one"),
        "{msg}"
    );
}

#[test]
fn accent_groups_range_errors() {
    let msg = parse_err("accent-groups 0 5");
    assert!(
        msg.contains("accent-groups") && msg.contains("group") && msg.contains("1..=16"),
        "{msg}"
    );
    let msg = parse_err("accent-groups 3 17");
    assert!(msg.contains("1..=16") && msg.contains("17"), "{msg}");
    let msg = parse_err("accent-groups 3 5 accent=128");
    assert!(msg.contains("accent") && msg.contains("1..=127"), "{msg}");
    let msg = parse_err("accent-groups 3 5 rest=0");
    assert!(msg.contains("rest") && msg.contains("1..=127"), "{msg}");
}

#[test]
fn feldman_field_defaults() {
    assert_eq!(
        parse_chain("feldman-field"),
        vec![EffectSpec::FeldmanField {
            seed: 0,
            floor: 8,
            ceiling: 28,
            jitter: 4,
        }]
    );
}

#[test]
fn feldman_field_full_form() {
    assert_eq!(
        parse_chain("feldman-field seed=6 floor=6 ceiling=24 jitter=3"),
        vec![EffectSpec::FeldmanField {
            seed: 6,
            floor: 6,
            ceiling: 24,
            jitter: 3,
        }]
    );
}

#[test]
fn feldman_field_range_errors() {
    let msg = parse_err("feldman-field floor=0");
    assert!(
        msg.contains("feldman-field") && msg.contains("floor") && msg.contains("1..=127"),
        "{msg}"
    );
    let msg = parse_err("feldman-field ceiling=128");
    assert!(msg.contains("ceiling") && msg.contains("1..=127"), "{msg}");
    let msg = parse_err("feldman-field floor=30 ceiling=20");
    assert!(
        msg.contains("floor=30"),
        "floor must not exceed ceiling: {msg}"
    );
    let msg = parse_err("feldman-field jitter=21");
    assert!(msg.contains("jitter") && msg.contains("0..=20"), "{msg}");
}

#[test]
fn velocity_invert_defaults_to_the_middle_pivot() {
    assert_eq!(
        parse_chain("velocity-invert"),
        vec![EffectSpec::VelocityInvert { pivot: 64 }]
    );
}

#[test]
fn velocity_invert_pivot_out_of_range_is_rejected() {
    let msg = parse_err("velocity-invert pivot=0");
    assert!(
        msg.contains("velocity-invert") && msg.contains("pivot") && msg.contains("1..=127"),
        "{msg}"
    );
    let msg = parse_err("velocity-invert pivot=128");
    assert!(msg.contains("1..=127") && msg.contains("128"), "{msg}");
}

#[test]
fn velocity_router_rebases_channels() {
    assert_eq!(
        parse_chain("velocity-router soft=2 medium=3 loud=4"),
        vec![EffectSpec::VelocityRouter {
            low: 64,
            high: 96,
            soft_ch: 1,
            mid_ch: 2,
            loud_ch: 3,
        }]
    );
}

#[test]
fn velocity_router_full_form() {
    assert_eq!(
        parse_chain("velocity-router low=40 high=100 soft=1 medium=8 loud=16"),
        vec![EffectSpec::VelocityRouter {
            low: 40,
            high: 100,
            soft_ch: 0,
            mid_ch: 7,
            loud_ch: 15,
        }]
    );
}

#[test]
fn velocity_router_requires_all_three_channels() {
    let msg = parse_err("velocity-router medium=3 loud=4");
    assert!(
        msg.contains("soft"),
        "error should name the missing property: {msg}"
    );
    let msg = parse_err("velocity-router soft=2 loud=4");
    assert!(
        msg.contains("medium"),
        "error should name the missing property: {msg}"
    );
    let msg = parse_err("velocity-router soft=2 medium=3");
    assert!(
        msg.contains("loud"),
        "error should name the missing property: {msg}"
    );
}

#[test]
fn velocity_router_low_must_stay_below_high() {
    let msg = parse_err("velocity-router low=96 high=96 soft=2 medium=3 loud=4");
    assert!(
        msg.contains("velocity-router") && msg.contains("low=96") && msg.contains("high=96"),
        "equal bounds leave no middle band: {msg}"
    );
    let msg = parse_err("velocity-router low=100 high=64 soft=2 medium=3 loud=4");
    assert!(msg.contains("low=100"), "low above high: {msg}");
}

#[test]
fn velocity_router_range_errors() {
    let msg = parse_err("velocity-router low=0 soft=2 medium=3 loud=4");
    assert!(msg.contains("low") && msg.contains("1..=127"), "{msg}");
    let msg = parse_err("velocity-router high=128 soft=2 medium=3 loud=4");
    assert!(msg.contains("high") && msg.contains("1..=127"), "{msg}");
    let msg = parse_err("velocity-router soft=0 medium=3 loud=4");
    assert!(msg.contains("1..=16"), "channels are 1-based: {msg}");
    let msg = parse_err("velocity-router soft=2 medium=3 loud=17");
    assert!(msg.contains("1..=16") && msg.contains("17"), "{msg}");
}

#[test]
fn anti_accent_defaults() {
    assert_eq!(
        parse_chain("anti-accent"),
        vec![EffectSpec::AntiAccent {
            seed: 0,
            level: 30,
            every: TimeSpec::Millis(30000.0),
        }]
    );
}

#[test]
fn anti_accent_full_form_with_beats() {
    assert_eq!(
        parse_chain("anti-accent level=20 beats=8 seed=2"),
        vec![EffectSpec::AntiAccent {
            seed: 2,
            level: 20,
            every: TimeSpec::Beats(8.0),
        }]
    );
}

#[test]
fn anti_accent_every_below_a_second_is_rejected() {
    let msg = parse_err("anti-accent every=\"500ms\"");
    assert!(
        msg.contains("anti-accent") && msg.contains("every") && msg.contains("at least 1s"),
        "{msg}"
    );
}

#[test]
fn anti_accent_level_out_of_range_is_rejected() {
    let msg = parse_err("anti-accent level=0");
    assert!(msg.contains("level") && msg.contains("1..=127"), "{msg}");
    let msg = parse_err("anti-accent level=128");
    assert!(msg.contains("1..=127") && msg.contains("128"), "{msg}");
}

#[test]
fn mass_crescendo_defaults() {
    assert_eq!(
        parse_chain("mass-crescendo"),
        vec![EffectSpec::MassCrescendo {
            period: TimeSpec::Millis(120000.0),
            depth: 0.6,
            shape: CrescendoShape::Arch,
        }]
    );
}

#[test]
fn mass_crescendo_full_form() {
    assert_eq!(
        parse_chain("mass-crescendo period=\"60s\" depth=0.4 shape=\"ramp\""),
        vec![EffectSpec::MassCrescendo {
            period: TimeSpec::Millis(60000.0),
            depth: 0.4,
            shape: CrescendoShape::Ramp,
        }]
    );
}

#[test]
fn mass_crescendo_period_below_a_second_is_rejected() {
    let msg = parse_err("mass-crescendo period=\"900ms\"");
    assert!(
        msg.contains("mass-crescendo") && msg.contains("period") && msg.contains("at least 1s"),
        "{msg}"
    );
}

#[test]
fn mass_crescendo_depth_out_of_range_is_rejected() {
    let msg = parse_err("mass-crescendo depth=1.5");
    assert!(msg.contains("depth") && msg.contains("0..=1"), "{msg}");
    let msg = parse_err("mass-crescendo depth=-0.1");
    assert!(msg.contains("0..=1"), "lower bound: {msg}");
}

#[test]
fn mass_crescendo_bad_shape_is_rejected() {
    let msg = parse_err("mass-crescendo shape=\"sawtooth\"");
    assert!(
        msg.contains("sawtooth") && msg.contains("ramp") && msg.contains("arch"),
        "error should show the bad shape and the alternatives: {msg}"
    );
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

#[test]
fn float_properties_accept_integer_literals() {
    // Every float-valued property takes a bare integer as well as a
    // decimal, so nobody has to know which spelling a node wants.
    let cfg = parse(
        "loose-keys seed=1 sigma=7\nvelocity-curve gamma=2\necho repeats=2 time=\"100ms\" decay=1\nwedge-mirror probability=1\nvelocity-dice seed=1 sigma=15\nresonance-halo level=1 decay=\"1s\"",
    );
    assert_eq!(cfg.scenes[0].chain.len(), 6);
}
