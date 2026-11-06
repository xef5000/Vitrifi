# Vitrifi for Visual Studio Code

Syntax highlighting for the `.vitrifi` architecture language, using your existing VS Code theme.

The extension recognizes authored targets, generated feature views, and the earlier authored language. It highlights feature names, imports, typed operation signatures, requirements, resources, effects, relationships, host bindings, strings, numbers, and comments. It also supplies comment toggling, bracket and quote pairing, indentation, folding, and snippets for common declarations.

## Install

In VS Code, run **Extensions: Install from VSIX…** from the Command Palette and select the generated `dist/vitrifi-0.3.0.vsix` file. Open a `.vitrifi` file; the language indicator should say **Vitrifi**. If an existing window retains the old language mode, use **Developer: Reload Window**.

The local extension ID is `vitrifi-local.vitrifi`. This package has not been published to the Marketplace.

## Authoring targets

Files ending in `.target.vitrifi` use the same language mode. Type `target` or `target-feature` to insert a feature target, or `target-operation` to add an operation with exact Java types:

```vitrifi
target feature Checkout.Discounts {
  purpose "Preview a cart without writes.";
  cover java "example.DiscountService";
  resource Carts table "carts";
  boundary Workflow java ["example.DiscountService"];
  boundary Storage java ["example.JdbcCartRepository"];

  operation preview(cartId: "java.lang.String") -> "example.Cart" {
    bind java "example.DiscountService.preview(java.lang.String)";
    effects [read Carts];
    dependencies [Storage];
    forbid [write Carts];
  }
}
```

The native type and symbol strings identify Java types and overloads. Coverage, table mappings, dependency boundaries, and the `forbid` / `no_effect_on_error` policies have distinct highlighting. These declarations constrain implementation; highlighting alone does not validate their bindings or establish conformance.

## Develop and package

From this directory, with Node.js 22+ and npm:

```sh
npm ci
npm test
mkdir -p dist
npm run package
```

The tests run the real VS Code TextMate/Oniguruma tokenizer against grammar fixtures and the repository's feature examples.

This is a declarative language extension: it has no activation script and does not launch the Vitrifi compiler or execute verification commands. Compiler diagnostics and navigation would be a separate language-server integration.

The grammar is reusable by TextMate-compatible tools. GitHub.com requires separate registration through GitHub Linguist; installing this extension does not change GitHub's rendering.
