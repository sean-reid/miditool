# Changelog

## [0.1.13](https://github.com/sean-reid/miditool/compare/v0.1.12...v0.1.13) (2026-07-14)


### Bug Fixes

* seed luau's math.random, correct gesture reload comments ([307128b](https://github.com/sean-reid/miditool/commit/307128b1c2f8a45fe493a2d3e14a69b608e619c9))

## [0.1.12](https://github.com/sean-reid/miditool/compare/v0.1.11...v0.1.12) (2026-07-13)


### Features

* config control block and form effect nodes ([049e538](https://github.com/sean-reid/miditool/commit/049e5381cd7568f027744c4fa0868bd90296a5db))
* crippled looper and retrograde buffer ([8bc9885](https://github.com/sean-reid/miditool/commit/8bc988576026754b5fb58826d7444aebb86a4d5f))
* keyboard gesture control and the moments sequencer ([ec4b8e1](https://github.com/sean-reid/miditool/commit/ec4b8e106214155972747722e1ce7d1b5f3dbf52))
* snap, a quantizer that follows the player's pulse ([c68220c](https://github.com/sean-reid/miditool/commit/c68220c5b513fd27d34920b18e137657d18898c6))

## [0.1.11](https://github.com/sean-reid/miditool/compare/v0.1.10...v0.1.11) (2026-07-06)


### Features

* config nodes and help for the generator wave ([4d38ccf](https://github.com/sean-reid/miditool/commit/4d38ccfef52ecadf930558d48eb69fa54c70d1a3))
* five free-running generators ([e68f162](https://github.com/sean-reid/miditool/commit/e68f162c751c08b78e428a725ce2c016636cddb7))
* the graph runs on a ticking thread, generators play without input ([0dbafdf](https://github.com/sean-reid/miditool/commit/0dbafdfbcc61cf4120d475554bd2de58fd77d71e))
* tick pass for free-running effects ([bdec8bb](https://github.com/sean-reid/miditool/commit/bdec8bb9a98bf14e99c0f4ceab0f13efbfbfc45d))


### Bug Fixes

* mark release prs tagged once dist creates the tag ([f94af7b](https://github.com/sean-reid/miditool/commit/f94af7b32ae90a44ff8ad3890448878d5c9d90d7))

## [0.1.10](https://github.com/sean-reid/miditool/compare/v0.1.9...v0.1.10) (2026-07-06)


### Bug Fixes

* cargo-dist creates the tag and release atomically with the binaries ([56bfbe4](https://github.com/sean-reid/miditool/commit/56bfbe4c9fec4e84c667ae9e36fde9e9cc9f8788))

## [0.1.9](https://github.com/sean-reid/miditool/compare/v0.1.8...v0.1.9) (2026-07-06)


### Bug Fixes

* defer release pr generation until the previous release is tagged ([337f9d5](https://github.com/sean-reid/miditool/commit/337f9d512967efab6012c321f5eacf04fdec45eb))

### Also in this release

* mpe voice pool and four microtonal effects: spectral-halo, just, scordatura, overtone-pedal ([0f0a4eb](https://github.com/sean-reid/miditool/commit/0f0a4eb))
* config nodes and help for the microtonal wave ([644711c](https://github.com/sean-reid/miditool/commit/644711c))

## [0.1.8](https://github.com/sean-reid/miditool/compare/v0.1.7...v0.1.8) (2026-07-06)


### Features

* config nodes and help for the harmony wave ([a083cee](https://github.com/sean-reid/miditool/commit/a083ceef913d55670aa52d0f1539f277a7619def))
* five harmonization effects ([0672d20](https://github.com/sean-reid/miditool/commit/0672d2038771a5876acdd364a3804d93c03ed7da))

## [0.1.7](https://github.com/sean-reid/miditool/compare/v0.1.6...v0.1.7) (2026-07-05)


### Bug Fixes

* draft releases until binaries are uploaded ([59e0c26](https://github.com/sean-reid/miditool/commit/59e0c26067158738eeea16af76e281f6d6826206))

## [0.1.6](https://github.com/sean-reid/miditool/compare/v0.1.5...v0.1.6) (2026-07-05)


### Features

* config nodes and help for the rhythm wave ([d52f605](https://github.com/sean-reid/miditool/commit/d52f6056ab5127b827e5ce333200c3b9f3a3a200))
* ten time, rhythm, and dynamics effects ([056fbf8](https://github.com/sean-reid/miditool/commit/056fbf8e12b8723b8d3d356f5f700ce470dd93ed))


### Bug Fixes

* deploy docs from a branch, sidestepping the flaky pages api ([88ce1f3](https://github.com/sean-reid/miditool/commit/88ce1f3911ceae6b1be976bab06131ca6b5283f2))

## [0.1.5](https://github.com/sean-reid/miditool/compare/v0.1.4...v0.1.5) (2026-07-05)


### Features

* config nodes and help for the stochastic wave ([0e45bab](https://github.com/sean-reid/miditool/commit/0e45bab2b7df6544c30b66915a28a19b5464b30f))
* home config with first-run creation and resolution order ([979e2db](https://github.com/sean-reid/miditool/commit/979e2db05df0841c267ec24e8e3d49d1433fd63a))
* seven stochastic and cluster effects ([049b560](https://github.com/sean-reid/miditool/commit/049b5609a1292f9d48e6543072d400eb5b8f2577))


### Bug Fixes

* float config properties accept integer literals ([770d353](https://github.com/sean-reid/miditool/commit/770d3536d091cafba304aa19aa26ea42d9e48de3))

## [0.1.4](https://github.com/sean-reid/miditool/compare/v0.1.3...v0.1.4) (2026-07-05)


### Features

* config nodes and help for the pitch and serial wave ([8ac8ca9](https://github.com/sean-reid/miditool/commit/8ac8ca9e312305ae50908e25228f4373078f03a7))
* nine pitch and serial effects with a sieve parser ([654167e](https://github.com/sean-reid/miditool/commit/654167e6b822c32ba5b525bef5d3642bb30b8629))

## [0.1.3](https://github.com/sean-reid/miditool/compare/v0.1.2...v0.1.3) (2026-07-05)


### Features

* luau scripting engine with sandboxing and fail-open safety ([444da3b](https://github.com/sean-reid/miditool/commit/444da3be7a0f75d7d19b6c45a54eac260c367f2d))
* script config node, new command, and scripting guide ([d7d4447](https://github.com/sean-reid/miditool/commit/d7d444714ca4537613dd9a9b479eb692b17bb1aa))


### Bug Fixes

* retry a rejected pages deployment once ([33847ba](https://github.com/sean-reid/miditool/commit/33847bae9299201306167024fbcda0462e4ceecc))

## [0.1.2](https://github.com/sean-reid/miditool/compare/v0.1.1...v0.1.2) (2026-07-05)


### Features

* remote binds loopback by default with a bind option and connection cap ([2f6d5e7](https://github.com/sean-reid/miditool/commit/2f6d5e71cdada3f89d1a31049aa5ddce67426014))


### Bug Fixes

* fork merges deleted delayed copies, silence stopped at one buffer, pairs could split ([2b8ee9c](https://github.com/sean-reid/miditool/commit/2b8ee9cab3b8a43aa0870b41abba655f2da325ff))
* let the newest docs deploy supersede a queued one ([76e4da2](https://github.com/sean-reid/miditool/commit/76e4da2b38a18fb84967b37a485f1baf85219f35))
* pedal release, aftertouch routing, sysex passthrough, and reload race ([9f31f48](https://github.com/sean-reid/miditool/commit/9f31f4859170426f75573116666ccf64663eb17f))
* remote and cli polish from the ux audit ([3c6b1a8](https://github.com/sean-reid/miditool/commit/3c6b1a8261661277123092557335184cf5b8c128))

## [0.1.1](https://github.com/sean-reid/miditool/compare/v0.1.0...v0.1.1) (2026-07-05)


### Bug Fixes

* dispatch the binary build when a release is tagged ([fe111f3](https://github.com/sean-reid/miditool/commit/fe111f38adeb5e867bda5bb7ea932dfdbc305ad2))
* upload release assets to the existing github release ([e559c24](https://github.com/sean-reid/miditool/commit/e559c2441386584b0695032f4b3fa3fec7d4d112))

## 0.1.0 (2026-07-05)


### Features

* cli with run, ports, monitor, and effects commands ([ea34b4f](https://github.com/sean-reid/miditool/commit/ea34b4ff697c37dc98374798a297fbbda2d272ea))
* delay, echo, restrike, and stutter effects ([92a3c83](https://github.com/sean-reid/miditool/commit/92a3c8311e654266bf5d46e97e7eb5dec6431e7f))
* founding effects with note-off routing ([1839e16](https://github.com/sean-reid/miditool/commit/1839e1691599b4551ecea1d133b17fad456f6704))
* hide the raw input from other apps while running (macos) ([2921370](https://github.com/sean-reid/miditool/commit/2921370eda684e686859d68cc3037b0ab350bc2b))
* kdl config format with examples ([b4d8ded](https://github.com/sean-reid/miditool/commit/b4d8ded3875d5e64004e0238b8fed940485329a9))
* live scene switching with kill and let-ring exit policies ([8eb77e8](https://github.com/sean-reid/miditool/commit/8eb77e84cb70b863b102e0de03cf69d0c0ca32f1))
* midi io layer and realtime engine pipeline ([9920955](https://github.com/sean-reid/miditool/commit/99209550afaa707688a5e954e5af1d430a75061a))
* realtime scheduler with hot reload and note draining ([c22869c](https://github.com/sean-reid/miditool/commit/c22869c76822b98feba2e1fa58a08e92025bcae6))
* release pipeline with versioning, changelog, and installers ([83db617](https://github.com/sean-reid/miditool/commit/83db6172e420510b44c526748993913e470d5d93))
* scene blocks and remote node in config ([bf5a16e](https://github.com/sean-reid/miditool/commit/bf5a16eb1639949e8e0582b6622b811475377962))
* support windows via winmm and loopmidi ([8597dc1](https://github.com/sean-reid/miditool/commit/8597dc100f5278723092484e8e80d7502e94c8a9))
* tempo node and duration syntax in config ([a394421](https://github.com/sean-reid/miditool/commit/a394421c2ad182afd8c89455875cd70f7004ee45))
* web remote with scene switching, live monitor, and panic ([65d98a9](https://github.com/sean-reid/miditool/commit/65d98a990f781c0e584eaa1f456a29028e171ccc))
* wire scenes and the web remote into run ([e1ce02c](https://github.com/sean-reid/miditool/commit/e1ce02c8835fce3431f4f20c2eedb65124fede04))
* wire scheduler and hot reload into run, add bench and doctor ([9a6e1e2](https://github.com/sean-reid/miditool/commit/9a6e1e2636b6814af57e1eab3cd7da16f6d40f0f))
* workspace scaffold and core event/graph model ([d714f54](https://github.com/sean-reid/miditool/commit/d714f54e9d8a5d828ef4b427504473f54942e3f1))


### Bug Fixes

* astro 7 needs node 22 in the docs build ([ba01cb1](https://github.com/sean-reid/miditool/commit/ba01cb10bbaf36ab3690889d5ca6be9eb6bf664c))
* deflake scheduler timing tests, add dbus headers to linux ci ([8664d24](https://github.com/sean-reid/miditool/commit/8664d245f6b2b13650d74ac5f9cc441141223065))
