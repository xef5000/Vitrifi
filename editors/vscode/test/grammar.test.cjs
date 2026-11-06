const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { before, test } = require('node:test');
const textmate = require('vscode-textmate');
const oniguruma = require('vscode-oniguruma');

const extension = path.resolve(__dirname, '..');
let grammar;

before(async () => {
  const wasm = fs.readFileSync(require.resolve('vscode-oniguruma/release/onig.wasm'));
  await oniguruma.loadWASM(wasm.buffer.slice(wasm.byteOffset, wasm.byteOffset + wasm.byteLength));
  const registry = new textmate.Registry({
    onigLib: Promise.resolve({
      createOnigScanner: patterns => new oniguruma.OnigScanner(patterns),
      createOnigString: value => new oniguruma.OnigString(value),
    }),
    loadGrammar: async scope => {
      assert.equal(scope, 'source.vitrifi');
      const file = path.join(extension, 'syntaxes/vitrifi.tmLanguage.json');
      return textmate.parseRawGrammar(fs.readFileSync(file, 'utf8'), file);
    },
  });
  grammar = await registry.loadGrammar('source.vitrifi');
});

function tokenize(source) {
  let stack = textmate.INITIAL;
  const lines = source.split('\n').map(text => {
    const result = grammar.tokenizeLine(text, stack);
    stack = result.ruleStack;
    return { text, tokens: result.tokens };
  });
  return { lines, stack };
}

function scopesAt(document, lineText, tokenText, occurrence = 0) {
  const line = document.lines.find(line => line.text.includes(lineText));
  assert.ok(line, `Missing fixture line: ${lineText}`);
  let offset = -1;
  for (let i = 0; i <= occurrence; i++) offset = line.text.indexOf(tokenText, offset + 1);
  assert.notEqual(offset, -1, `Missing ${tokenText} in ${line.text}`);
  const token = line.tokens.find(token => token.startIndex <= offset && offset < token.endIndex);
  assert.ok(token, `No token at ${offset} in ${line.text}`);
  return token.scopes;
}

function has(document, line, text, expected, occurrence = 0) {
  const scopes = scopesAt(document, line, text, occurrence);
  assert.ok(scopes.includes(expected), `${JSON.stringify(text)} expected ${expected}; got ${scopes.join(', ')}`);
}

function lacks(document, line, text, prefix, occurrence = 0) {
  const scopes = scopesAt(document, line, text, occurrence);
  assert.ok(!scopes.some(scope => scope.startsWith(prefix)), `${JSON.stringify(text)} unexpectedly has ${prefix}: ${scopes.join(', ')}`);
}

test('feature imports, declarations, typed signatures and relationship targets use theme-compatible scopes', () => {
  const document = tokenize(`use "./storage/cart.vitrifi" as Store;
feature Checkout.Discounts {
  export type Receipt;
  resource Cart;
  requirement Owner "Only the owner.";
  boundary Domain ["./*.py"] allows [Store.Storage];
  operation apply(actor: String, carts: List<Store.Cart>) -> Result<Receipt, Store.Error> {
    bind python "./discounts.py" symbol "apply";
    effects [read Store.Carts, write Cart];
    requires [Owner];
    calls [Store.save];
  }
  check Behavior { run ["python3"]; cwd "."; covers [Owner]; timeout 30; }
}`);
  has(document, 'use ', 'use', 'keyword.control.import.vitrifi');
  has(document, 'use ', 'Store', 'entity.name.namespace.vitrifi');
  has(document, 'feature ', 'Checkout', 'entity.name.namespace.vitrifi');
  has(document, 'export type', 'export', 'storage.modifier.export.vitrifi');
  has(document, 'export type', 'Receipt', 'entity.name.type.vitrifi');
  has(document, 'resource Cart', 'Cart', 'variable.other.readwrite.resource.vitrifi');
  has(document, 'requirement Owner', 'Owner', 'variable.other.constant.requirement.vitrifi');
  has(document, 'boundary Domain', 'Domain', 'entity.name.type.class.boundary.vitrifi');
  has(document, 'operation apply', 'apply', 'entity.name.function.vitrifi');
  has(document, 'operation apply', 'actor', 'variable.parameter.vitrifi');
  has(document, 'operation apply', 'String', 'support.type.primitive.vitrifi');
  has(document, 'operation apply', 'Receipt', 'entity.name.type.vitrifi');
  has(document, 'bind python', 'python', 'support.constant.language.vitrifi');
  has(document, 'bind python', 'symbol', 'keyword.other.binding.vitrifi');
  has(document, 'effects [', 'read', 'keyword.operator.effect.vitrifi');
  has(document, 'effects [', 'Carts', 'variable.other.readwrite.resource.vitrifi');
  has(document, 'calls [', 'save', 'entity.name.function.vitrifi');
  has(document, 'requires [', 'Owner', 'variable.other.constant.requirement.vitrifi');
  has(document, 'check Behavior', 'Behavior', 'entity.name.function.check.vitrifi');
  has(document, 'check Behavior', '30', 'constant.numeric.integer.vitrifi');
  assert.equal(document.stack.depth, 1);
});

test('nested generics and comments between signature tokens preserve parameter and body context', () => {
  const document = tokenize(`feature Example {
  operation /* declaration comment */
    apply (
      actor /* parameter comment */ : Result<
        List<Option<String>>, Store.Cart
      >,
      next: Int,
    ) /* result comment */ ->
      List<Result<Store.Cart, String>>
    {
      bind /* language comment */ typescript "./app.ts" symbol "apply";
    }
  resource Later;
}`);
  has(document, '    apply (', 'apply', 'entity.name.function.vitrifi');
  has(document, 'actor /*', 'actor', 'variable.parameter.vitrifi');
  has(document, 'next: Int', 'next', 'variable.parameter.vitrifi');
  has(document, 'List<Option', 'String', 'support.type.primitive.vitrifi');
  has(document, 'bind /*', 'typescript', 'support.constant.language.vitrifi');
  has(document, 'resource Later', 'Later', 'variable.other.readwrite.resource.vitrifi');
  assert.equal(document.stack.depth, 1);
});

test('authored targets distinguish coverage, native types, resource mappings, dependencies and policies', () => {
  const document = tokenize(`target feature Checkout.Discounts {
  purpose "A read-only preview.";
  cover java "example.DiscountService";
  resource Carts table "carts";
  boundary Workflow java ["example.DiscountService"];
  boundary Storage java ["example.JdbcCartRepository"];
  operation preview(cartId: "java.lang.String", amounts: "java.util.Map<java.lang.String, java.lang.Long>") -> "example.Cart" {
    bind java "example.DiscountService.preview(java.lang.String,java.util.Map)";
    effects [read Carts];
    dependencies [Storage];
    forbid [write Carts];
    no_effect_on_error [write Carts];
  }
  resource Later table "later";
}`);
  has(document, 'target feature', 'target', 'storage.modifier.target.vitrifi');
  has(document, 'target feature', 'feature', 'storage.type.feature.vitrifi');
  has(document, 'target feature', 'Checkout', 'entity.name.namespace.vitrifi');
  has(document, 'cover java', 'cover', 'keyword.control.coverage.vitrifi');
  has(document, 'cover java', 'java', 'support.constant.language.vitrifi');
  has(document, 'cover java', 'example.DiscountService', 'string.quoted.double.vitrifi');
  has(document, 'resource Carts', 'Carts', 'variable.other.readwrite.resource.vitrifi');
  has(document, 'resource Carts', 'table', 'keyword.other.resource-mapping.vitrifi');
  has(document, 'boundary Workflow', 'Workflow', 'entity.name.type.class.boundary.vitrifi');
  has(document, 'boundary Workflow', 'java', 'support.constant.language.vitrifi');
  has(document, 'operation preview', 'cartId', 'variable.parameter.vitrifi');
  has(document, 'operation preview', 'java.lang.String', 'string.quoted.double.vitrifi');
  has(document, 'operation preview', 'java.lang.Long', 'string.quoted.double.vitrifi');
  has(document, 'operation preview', 'example.Cart', 'meta.return-type.vitrifi');
  has(document, 'operation preview', 'example.Cart', 'string.quoted.double.vitrifi');
  lacks(document, 'operation preview', 'java.lang.Long', 'variable.parameter.');
  has(document, 'bind java', 'java', 'support.constant.language.vitrifi');
  has(document, 'bind java', 'example.DiscountService.preview', 'string.quoted.double.vitrifi');
  has(document, 'dependencies [', 'dependencies', 'keyword.control.relationship.vitrifi');
  has(document, 'dependencies [', 'Storage', 'entity.name.type.class.boundary.vitrifi');
  has(document, 'forbid [', 'forbid', 'keyword.control.policy.vitrifi');
  has(document, 'forbid [', 'write', 'keyword.operator.effect.vitrifi');
  has(document, 'forbid [', 'Carts', 'variable.other.readwrite.resource.vitrifi');
  has(document, 'no_effect_on_error [', 'no_effect_on_error', 'keyword.control.policy.vitrifi');
  has(document, 'resource Later', 'Later', 'variable.other.readwrite.resource.vitrifi');
  assert.equal(document.stack.depth, 1);
});

test('target keywords remain identifiers and comments do not break native declaration context', () => {
  const document = tokenize(`target /* mode */ feature target {
  cover /* adapter */ java "example.Service";
  resource table /* mapping */ table "records";
  boundary java /* adapter */ java ["example.Service"];
  boundary dependencies java ["example.Storage"];
  operation forbid(
    dependencies /* parameter */ : "java.util.List<java.lang.String>",
    cover: "int"
  ) -> /* result */ "long" {
    bind java "example.Service.forbid(java.util.List,int)";
    effects [read table];
    dependencies [dependencies];
    forbid [write table];
  }
}`);
  has(document, 'feature target', 'target', 'entity.name.namespace.vitrifi', 1);
  has(document, 'cover /*', 'java', 'support.constant.language.vitrifi');
  has(document, 'resource table', 'table', 'variable.other.readwrite.resource.vitrifi');
  has(document, 'resource table', 'table', 'keyword.other.resource-mapping.vitrifi', 1);
  has(document, 'boundary java', 'java', 'entity.name.type.class.boundary.vitrifi');
  has(document, 'boundary java', 'java', 'support.constant.language.vitrifi', 1);
  has(document, 'boundary dependencies', 'dependencies', 'entity.name.type.class.boundary.vitrifi');
  has(document, 'operation forbid', 'forbid', 'entity.name.function.vitrifi');
  has(document, 'dependencies /*', 'dependencies', 'variable.parameter.vitrifi');
  has(document, 'cover: "int"', 'int', 'string.quoted.double.vitrifi');
  has(document, ') -> /* result */', 'long', 'string.quoted.double.vitrifi');
  has(document, 'dependencies [', 'dependencies', 'entity.name.type.class.boundary.vitrifi', 1);
  lacks(document, 'dependencies [', 'dependencies', 'keyword.', 1);
  has(document, 'forbid [', 'table', 'variable.other.readwrite.resource.vitrifi');
  assert.equal(document.stack.depth, 1);
});

test('keyword spellings remain identifiers in declaration, parameter and relationship contexts', () => {
  const document = tokenize(`use "./other.vitrifi" as as;
feature feature {
  type operation;
  resource read;
  boundary allows ["./*.py"] allows [];
  operation read(bind: operation) -> operation {
    bind python "./app.py" symbol "read";
    effects [read read, write as.read];
    calls [read, as.type];
  }
  check run { run ["true"]; covers []; }
}`);
  has(document, 'use ', 'as', 'entity.name.namespace.vitrifi', 1);
  has(document, 'feature feature', 'feature', 'entity.name.namespace.vitrifi', 1);
  has(document, 'type operation', 'operation', 'entity.name.type.vitrifi');
  has(document, 'resource read', 'read', 'variable.other.readwrite.resource.vitrifi');
  has(document, 'boundary allows', 'allows', 'entity.name.type.class.boundary.vitrifi');
  has(document, 'operation read', 'read', 'entity.name.function.vitrifi');
  has(document, 'operation read', 'bind', 'variable.parameter.vitrifi');
  has(document, 'effects [', 'read', 'variable.other.readwrite.resource.vitrifi', 1);
  has(document, 'calls [', 'read', 'entity.name.function.vitrifi');
  has(document, 'calls [', 'type', 'entity.name.function.vitrifi');
  has(document, 'check run', 'run', 'entity.name.function.check.vitrifi');
  lacks(document, 'calls [', 'read', 'keyword.');
  assert.equal(document.stack.depth, 1);
});

test('multiline comments and string content do not become executable-looking declarations', () => {
  const document = tokenize(`/* feature Fake {
operation misleading() -> String { }
*/
feature Real {
  purpose "calls [write], operation fake, and // stay prose";
  // effects [write Fake];
  requirement Meaning "/* not a block comment */";
  resource Actual;
}`);
  has(document, 'operation misleading', 'operation', 'comment.block.vitrifi');
  lacks(document, 'operation misleading', 'operation', 'storage.type.');
  has(document, 'purpose "calls', 'calls', 'string.quoted.double.vitrifi');
  lacks(document, 'purpose "calls', 'calls', 'keyword.');
  has(document, '// effects', 'effects', 'comment.line.double-slash.vitrifi');
  has(document, 'requirement Meaning', '/*', 'string.quoted.double.vitrifi');
  has(document, 'resource Actual', 'Actual', 'variable.other.readwrite.resource.vitrifi');
  assert.equal(document.stack.depth, 1);
});

test('JSON string escapes distinguish valid and invalid escapes and do not bleed across lines', () => {
  const document = tokenize(String.raw`feature Strings {
  purpose "quote: \" newline: \n unicode: \u0041 invalid: \q";
  question "truncated \u12Z4";
  requirement Broken "not closed
  resource Afterwards;
}`);
  has(document, 'purpose ', String.raw`\"`, 'constant.character.escape.vitrifi');
  has(document, 'purpose ', String.raw`\n`, 'constant.character.escape.vitrifi');
  has(document, 'purpose ', String.raw`\u0041`, 'constant.character.escape.vitrifi');
  has(document, 'purpose ', String.raw`\q`, 'invalid.illegal.escape.vitrifi');
  has(document, 'question ', String.raw`\u`, 'invalid.illegal.escape.vitrifi');
  lacks(document, 'resource Afterwards', 'Afterwards', 'string.');
});

test('all real feature modules tokenize completely without leaked declaration scopes', () => {
  const root = path.resolve(extension, '../..');
  for (const relative of [
    'examples/checkout/discounts.vitrifi',
    'examples/checkout/storage/cart.vitrifi',
    'examples/checkout/audit/discount.vitrifi',
    'examples/spring-checkout/src/main/java/dev/vitrifi/example/checkout/Checkout.Discounts.vitrifi',
    'examples/spring-checkout/src/main/java/dev/vitrifi/example/checkout/Checkout.Discounts.target.vitrifi',
  ]) {
    const source = fs.readFileSync(path.join(root, relative), 'utf8');
    const document = tokenize(source);
    assert.equal(document.stack.depth, 1, `${relative} left an open grammar region`);
    const invalid = document.lines.flatMap(line => line.tokens.filter(token => token.scopes.some(scope => scope.startsWith('invalid.'))));
    assert.equal(invalid.length, 0, `${relative} unexpectedly contains invalid-highlight tokens`);
    assert.ok(document.lines.some(line => line.tokens.some(token => token.scopes.includes('entity.name.function.vitrifi'))));
  }
});
