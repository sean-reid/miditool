//! End-to-end tests for the public parsing API: the shipped examples, the
//! documented defaults, and the validation errors.

use miditool_config::{Config, EffectSpec, OutputSpec, ShuffleMode, parse_str};

fn parse(text: &str) -> Config {
    parse_str("test.kdl", text).expect("config should parse")
}

fn parse_err(text: &str) -> String {
    parse_str("test.kdl", text)
        .expect_err("config should not parse")
        .to_string()
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
            chain: vec![
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
            chain: vec![],
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
fn velocity_curve_defaults() {
    let config = parse("velocity-curve");
    assert_eq!(
        config.chain,
        vec![EffectSpec::VelocityCurve {
            gamma: 1.0,
            floor: 1,
            ceiling: 127,
        }]
    );
}

#[test]
fn shuffle_lock_defaults() {
    let config = parse("shuffle-lock seed=1");
    assert_eq!(
        config.chain,
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
    let config = parse(
        "shuffle-lock seed=1 mode=\"within-octave\"\n\
         shuffle-lock seed=2 mode=\"within-pitch-class\"",
    );
    assert_eq!(
        config.chain,
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
    let config = parse("loose-keys seed=3 lo=30 hi=90 sigma=7.0");
    assert_eq!(
        config.chain,
        vec![EffectSpec::LooseKeysGaussian {
            seed: 3,
            sigma: 7.0,
        }]
    );
}

#[test]
fn loose_keys_defaults_to_piano_range() {
    let config = parse("loose-keys seed=3");
    assert_eq!(
        config.chain,
        vec![EffectSpec::LooseKeysUniform {
            seed: 3,
            lo: 21,
            hi: 108,
        }]
    );
}

#[test]
fn channels_are_rebased_sorted_and_deduplicated() {
    let config = parse("only-channels 3 1 16 3");
    assert_eq!(config.chain, vec![EffectSpec::OnlyChannels(vec![0, 2, 15])]);
}

#[test]
fn negative_transpose() {
    let config = parse("transpose -12");
    assert_eq!(config.chain, vec![EffectSpec::Transpose { semis: -12 }]);
}

#[test]
fn fork_of_chains_of_filters_round_trips() {
    let config = parse(
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
        config.chain,
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
