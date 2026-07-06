// @ts-check
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

export default defineConfig({
  site: "https://sean-reid.github.io",
  base: "/miditool",
  // Bare section URLs land on each section's first page. Destinations
  // carry the base by hand: Astro does not prepend it for redirects.
  redirects: {
    "/guides": "/miditool/guides/garageband/",
    "/configuration": "/miditool/configuration/config-files/",
    "/effects": "/miditool/effects/shuffle-lock/",
    "/reference": "/miditool/reference/cli/",
  },
  integrations: [
    starlight({
      title: "miditool",
      description:
        "A MIDI mixing layer between your keyboard and your DAW: scrambled keys, stochastic effects, deterministic seeds, a phone remote.",
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/sean-reid/miditool",
        },
      ],
      customCss: ["./src/styles/custom.css"],
      sidebar: [
        {
          label: "Start here",
          items: [
            { label: "miditool", slug: "index" },
            { label: "Getting started", slug: "getting-started" },
            { label: "How it works", slug: "how-it-works" },
          ],
        },
        {
          label: "Guides",
          items: [
            { label: "GarageBand", slug: "guides/garageband" },
            { label: "Other DAWs", slug: "guides/daws" },
            { label: "The web remote", slug: "guides/remote" },
            { label: "Performing", slug: "guides/performing" },
            { label: "Live editing", slug: "guides/live-editing" },
            { label: "Microtonality", slug: "guides/microtonality" },
            { label: "Linux", slug: "guides/linux" },
            { label: "Windows", slug: "guides/windows" },
            { label: "Troubleshooting", slug: "guides/troubleshooting" },
          ],
        },
        {
          label: "Configuration",
          items: [
            { label: "Config files", slug: "configuration/config-files" },
            { label: "Routing and filters", slug: "configuration/routing" },
            { label: "Time and tempo", slug: "configuration/time" },
            { label: "Seeds", slug: "configuration/seeds" },
            { label: "Scripting", slug: "configuration/scripting" },
          ],
        },
        {
          label: "Effects",
          items: [
            { label: "shuffle-lock", slug: "effects/shuffle-lock" },
            { label: "loose-keys", slug: "effects/loose-keys" },
            { label: "transpose", slug: "effects/transpose" },
            { label: "wedge-mirror", slug: "effects/wedge-mirror" },
            { label: "telescope", slug: "effects/telescope" },
            { label: "registral-scatter", slug: "effects/registral-scatter" },
            { label: "blocked-keys", slug: "effects/blocked-keys" },
            { label: "row-snap", slug: "effects/row-snap" },
            { label: "aggregate-gate", slug: "effects/aggregate-gate" },
            { label: "sieve", slug: "effects/sieve" },
            { label: "note-roulette", slug: "effects/note-roulette" },
            { label: "poisson-cloud", slug: "effects/poisson-cloud" },
            { label: "duration-lottery", slug: "effects/duration-lottery" },
            { label: "density-governor", slug: "effects/density-governor" },
            { label: "ring-mod", slug: "effects/ring-mod" },
            { label: "tintinnabuli", slug: "effects/tintinnabuli" },
            { label: "mode-lock", slug: "effects/mode-lock" },
            { label: "negative-harmony", slug: "effects/negative-harmony" },
            { label: "tonnetz", slug: "effects/tonnetz" },
            { label: "complement-pad", slug: "effects/complement-pad" },
            { label: "spectral-halo", slug: "effects/spectral-halo" },
            { label: "just", slug: "effects/just" },
            { label: "scordatura", slug: "effects/scordatura" },
            { label: "overtone-pedal", slug: "effects/overtone-pedal" },
            { label: "continuum", slug: "effects/continuum" },
            { label: "metronome-swarm", slug: "effects/metronome-swarm" },
            { label: "brownian-walker", slug: "effects/brownian-walker" },
            { label: "mechanico", slug: "effects/mechanico" },
            { label: "continuator", slug: "effects/continuator" },
            { label: "crippled-looper", slug: "effects/crippled-looper" },
            { label: "retrograde", slug: "effects/retrograde" },
            { label: "cluster-fist", slug: "effects/cluster-fist" },
            { label: "resonance-halo", slug: "effects/resonance-halo" },
            { label: "velocity-curve", slug: "effects/velocity-curve" },
            { label: "velocity-dice", slug: "effects/velocity-dice" },
            { label: "accent-groups", slug: "effects/accent-groups" },
            { label: "feldman-field", slug: "effects/feldman-field" },
            { label: "velocity-invert", slug: "effects/velocity-invert" },
            { label: "velocity-router", slug: "effects/velocity-router" },
            { label: "anti-accent", slug: "effects/anti-accent" },
            { label: "mass-crescendo", slug: "effects/mass-crescendo" },
            { label: "channelize", slug: "effects/channelize" },
            { label: "klangfarben", slug: "effects/klangfarben" },
            { label: "delay", slug: "effects/delay" },
            { label: "echo", slug: "effects/echo" },
            { label: "restrike", slug: "effects/restrike" },
            { label: "stutter", slug: "effects/stutter" },
            { label: "euclidean-gate", slug: "effects/euclidean-gate" },
            { label: "quantize", slug: "effects/quantize" },
            { label: "talea", slug: "effects/talea" },
            { label: "added-value", slug: "effects/added-value" },
          ],
        },
        {
          label: "Reference",
          items: [
            { label: "CLI", slug: "reference/cli" },
            { label: "Releasing", slug: "reference/releasing" },
          ],
        },
      ],
    }),
  ],
});
