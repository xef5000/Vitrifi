# Vitrifi explainer

A small, standalone static website illustrating the Java/Spring generated workflow. Open `index.html` directly in a browser; no server, install, build step, remote fonts, or external scripts are required.

From the repository root on macOS:

```sh
open site/index.html
```

The page includes four before/after examples, an interactive guide to generated syntax, scope explanations, and commands for the real Spring/H2 example. The interactive results are prewritten explanatory snapshots based on the repository's end-to-end mutation checks. They do not run Java or Vitrifi in the browser; terminal output is condensed for readability.

Files:

- `index.html`: content and accessible page structure.
- `style.css`: responsive layout and local syntax colors.
- `app.js`: example switching, code highlighting, glossary, and clipboard behavior.
- `examples.js`: source excerpts and expected outcomes.
- `examples/Checkout.Discounts.vitrifi`: an unmodified copy of the generated example for download. Refresh it when the source fixture changes.

To host it, upload the contents of this directory to any static host. All assets use relative paths, so the page also works in a subdirectory. No deployment is configured or required.
