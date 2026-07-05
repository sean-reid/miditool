// @ts-check
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

export default defineConfig({
  site: "https://sean-reid.github.io",
  base: "/miditool",
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
            { label: "Live editing", slug: "guides/live-editing" },
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
          ],
        },
        {
          label: "Effects",
          items: [
            { label: "shuffle-lock", slug: "effects/shuffle-lock" },
            { label: "loose-keys", slug: "effects/loose-keys" },
            { label: "transpose", slug: "effects/transpose" },
            { label: "velocity-curve", slug: "effects/velocity-curve" },
            { label: "channelize", slug: "effects/channelize" },
            { label: "delay", slug: "effects/delay" },
            { label: "echo", slug: "effects/echo" },
            { label: "restrike", slug: "effects/restrike" },
            { label: "stutter", slug: "effects/stutter" },
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
