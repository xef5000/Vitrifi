# Vitrifi explainer

A small, standalone static website illustrating Java/Spring architecture analysis, compiled workflows and algebraic proof certificates. Open `index.html` directly in a browser; no server, install, build step, remote fonts, or external scripts are required.

From the repository root on macOS:

```sh
open site/index.html
```

The page includes:

- A typed workflow excerpt and a guide to the six authored/generated file roles.
- Five before/after architecture examples, including a write-before-rejection error caught before Java generation.
- An interactive guide to the analyzer-generated syntax from the original annotation-based checkout.
- Four proof cases: a certified flat-to-nested refactor, an extra action invocation, a changed rejection message and changed native business logic.
- The supported algebraic laws, pinned assumptions and practical proof limits, plus commands for the real Spring/H2 demonstration.

Interactive results are prewritten explanatory snapshots based on `scripts/test-compiled-workflow.py`, `scripts/test-workflow-proofs.py` and the Java mutation suites. They do not run Java or Vitrifi in the browser; terminal output is condensed. The proof report shows selected fields from an actual successful checker result. `PROOF_NOT_FOUND` means the supported search did not find a derivation, not a general proof of inequivalence.

Files:

- `index.html`: content and accessible page structure.
- `style.css`: responsive layout and local syntax colors.
- `app.js`: example switching, code highlighting, glossary, and clipboard behavior.
- `examples.js`: source excerpts and expected outcomes.
- `proofs.js`: baseline/candidate excerpts and expected certificate outcomes.
- `examples/Checkout.Discounts.vitrifi`: an unmodified copy of the generated `examples/spring-checkout` view for download.
- `examples/Checkout.Discounts.workflow.vitrifi` and `examples/Checkout.Discounts.target.vitrifi`: unmodified copies of the authored `examples/workflow-checkout` fixtures. Refresh downloads when their source fixtures change.

To host it, upload the contents of this directory to any static host. All assets use relative paths, so the page also works in a subdirectory. No deployment is configured or required.
