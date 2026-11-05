//! Deterministic parser for feature-local architecture modules.
//!
//! The language has no executable expressions: parsing a module never evaluates
//! host code, imports an implementation, or starts a verification command.
use crate::model::*;
use std::collections::HashSet;

const MAX_TYPE_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
enum TokenKind {
    Ident(String),
    String(String),
    Number(u64),
    Punctuation(char),
    Arrow,
    Eof,
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    location: Location,
}

struct Lexer<'a> {
    path: &'a str,
    source: &'a str,
    offset: usize,
    line: usize,
    column: usize,
}

impl<'a> Lexer<'a> {
    fn location(&self) -> Location {
        Location {
            path: self.path.into(),
            line: self.line,
            column: self.column,
        }
    }

    fn peek(&self) -> Option<char> {
        self.source[self.offset..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.offset += ch.len_utf8();
        if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(ch)
    }

    fn starts(&self, text: &str) -> bool {
        self.source[self.offset..].starts_with(text)
    }

    fn error(&self, location: Location, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            code: "LEX_ERROR".into(),
            message: message.into(),
            location,
        }
    }

    fn tokenize(mut self) -> Result<Vec<Token>, Diagnostic> {
        let mut tokens = Vec::new();
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.bump();
                continue;
            }
            if self.starts("//") {
                while self.peek().is_some_and(|c| c != '\n') {
                    self.bump();
                }
                continue;
            }
            if self.starts("/*") {
                let start = self.location();
                self.bump();
                self.bump();
                while !self.starts("*/") {
                    if self.bump().is_none() {
                        return Err(self.error(start, "Unterminated block comment"));
                    }
                }
                self.bump();
                self.bump();
                continue;
            }
            let location = self.location();
            let kind = if ch.is_ascii_alphabetic() || ch == '_' {
                let start = self.offset;
                while self
                    .peek()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                {
                    self.bump();
                }
                TokenKind::Ident(self.source[start..self.offset].into())
            } else if ch.is_ascii_digit() {
                let start = self.offset;
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.bump();
                }
                let value = self.source[start..self.offset]
                    .parse::<u64>()
                    .map_err(|_| {
                        self.error(location.clone(), "Integer exceeds the supported u64 range")
                    })?;
                TokenKind::Number(value)
            } else if ch == '"' {
                let start = self.offset;
                self.bump();
                loop {
                    match self.bump() {
                        Some('"') => break,
                        Some('\\') => {
                            if self.bump().is_none() {
                                return Err(self.error(location, "Unterminated string escape"));
                            }
                        }
                        Some('\n' | '\r') => {
                            return Err(
                                self.error(location, "Strings cannot contain an unescaped newline")
                            )
                        }
                        None => return Err(self.error(location, "Unterminated string")),
                        _ => {}
                    }
                }
                let value: String = serde_json::from_str(&self.source[start..self.offset])
                    .map_err(|error| {
                        self.error(location.clone(), format!("Invalid string literal: {error}"))
                    })?;
                TokenKind::String(value)
            } else if self.starts("->") {
                self.bump();
                self.bump();
                TokenKind::Arrow
            } else if "{}[](),;:.<>".contains(ch) {
                self.bump();
                TokenKind::Punctuation(ch)
            } else {
                return Err(self.error(location, format!("Unexpected character {ch:?}")));
            };
            tokens.push(Token { kind, location });
        }
        tokens.push(Token {
            kind: TokenKind::Eof,
            location: self.location(),
        });
        Ok(tokens)
    }
}

struct Parser {
    path: String,
    tokens: Vec<Token>,
    cursor: usize,
}

impl Parser {
    fn token(&self) -> &Token {
        &self.tokens[self.cursor]
    }

    fn advance(&mut self) -> Token {
        let token = self.token().clone();
        if token.kind != TokenKind::Eof {
            self.cursor += 1;
        }
        token
    }

    fn error(&self, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            code: "PARSE_ERROR".into(),
            message: message.into(),
            location: self.token().location.clone(),
        }
    }

    fn at_error(&self, location: Location, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            code: "PARSE_ERROR".into(),
            message: message.into(),
            location,
        }
    }

    fn is_keyword(&self, keyword: &str) -> bool {
        matches!(&self.token().kind, TokenKind::Ident(text) if text == keyword)
    }

    fn eat_keyword(&mut self, keyword: &str) -> bool {
        if self.is_keyword(keyword) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn keyword(&mut self, keyword: &str) -> Result<(), Diagnostic> {
        if self.eat_keyword(keyword) {
            Ok(())
        } else {
            Err(self.error(format!("Expected '{keyword}'")))
        }
    }

    fn eat(&mut self, punctuation: char) -> bool {
        if self.token().kind == TokenKind::Punctuation(punctuation) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn punctuation(&mut self, punctuation: char) -> Result<(), Diagnostic> {
        if self.eat(punctuation) {
            Ok(())
        } else {
            Err(self.error(format!("Expected '{punctuation}'")))
        }
    }

    fn ident(&mut self) -> Result<(String, Location), Diagnostic> {
        match &self.token().kind {
            TokenKind::Ident(_) => {
                let token = self.advance();
                let TokenKind::Ident(name) = token.kind else {
                    unreachable!()
                };
                Ok((name, token.location))
            }
            _ => Err(self.error("Expected an identifier")),
        }
    }

    fn reference(&mut self) -> Result<Reference, Diagnostic> {
        let (mut name, location) = self.ident()?;
        while self.eat('.') {
            name.push('.');
            name.push_str(&self.ident()?.0);
        }
        Ok(Reference { name, location })
    }

    fn string(&mut self) -> Result<String, Diagnostic> {
        if let TokenKind::String(value) = self.token().kind.clone() {
            self.advance();
            Ok(value)
        } else {
            Err(self.error("Expected a double-quoted string"))
        }
    }

    fn number(&mut self) -> Result<u64, Diagnostic> {
        if let TokenKind::Number(value) = self.token().kind {
            self.advance();
            Ok(value)
        } else {
            Err(self.error("Expected a nonnegative integer"))
        }
    }

    fn list<T>(
        &mut self,
        mut item: impl FnMut(&mut Self) -> Result<T, Diagnostic>,
    ) -> Result<Vec<T>, Diagnostic> {
        self.punctuation('[')?;
        let mut values = Vec::new();
        if self.eat(']') {
            return Ok(values);
        }
        loop {
            values.push(item(self)?);
            if self.eat(']') {
                break;
            }
            self.punctuation(',')?;
            if self.eat(']') {
                break;
            }
        }
        Ok(values)
    }

    fn type_expr(&mut self, depth: usize) -> Result<TypeExpr, Diagnostic> {
        if depth >= MAX_TYPE_DEPTH {
            return Err(self.error(format!(
                "Type nesting exceeds the {MAX_TYPE_DEPTH}-level limit"
            )));
        }
        let reference = self.reference()?;
        let mut arguments = Vec::new();
        if self.eat('<') {
            loop {
                arguments.push(self.type_expr(depth + 1)?);
                if self.eat('>') {
                    break;
                }
                self.punctuation(',')?;
            }
        }
        Ok(TypeExpr {
            name: reference.name,
            arguments,
            location: reference.location,
        })
    }

    fn singleton(
        &self,
        seen: &mut HashSet<String>,
        name: &str,
        location: &Location,
    ) -> Result<(), Diagnostic> {
        if seen.insert(name.into()) {
            Ok(())
        } else {
            Err(self.at_error(location.clone(), format!("Duplicate '{name}' field")))
        }
    }

    fn operation(&mut self) -> Result<DeclBody, Diagnostic> {
        self.punctuation('(')?;
        let mut parameters = Vec::new();
        if !self.eat(')') {
            loop {
                let (name, _) = self.ident()?;
                self.punctuation(':')?;
                let ty = self.type_expr(0)?;
                parameters.push(Parameter { name, ty });
                if self.eat(')') {
                    break;
                }
                self.punctuation(',')?;
                if self.eat(')') {
                    break;
                }
            }
        }
        if self.token().kind != TokenKind::Arrow {
            return Err(self.error("Expected '->' before the operation result type"));
        }
        self.advance();
        let returns = self.type_expr(0)?;
        self.punctuation('{')?;
        let mut purpose = None;
        let mut binding = None;
        let mut effects = Vec::new();
        let mut requires = Vec::new();
        let mut ensures = Vec::new();
        let mut calls = Vec::new();
        let mut seen = HashSet::new();
        while !self.eat('}') {
            let (field, location) = self.ident()?;
            self.singleton(&mut seen, &field, &location)?;
            match field.as_str() {
                "purpose" => purpose = Some(self.string()?),
                "bind" => {
                    let language = self.ident()?.0;
                    let path = self.string()?;
                    self.keyword("symbol")?;
                    let symbol = self.string()?;
                    binding = Some(Binding {
                        language,
                        path,
                        symbol,
                        location,
                    });
                }
                "effects" => {
                    effects = self.list(|parser| {
                        let (action, location) = parser.ident()?;
                        if !matches!(action.as_str(), "read" | "write" | "network" | "emit") {
                            return Err(parser.at_error(location, format!("Unknown effect action '{action}'; expected read, write, network, or emit")));
                        }
                        Ok(Effect { action, resource: parser.reference()? })
                    })?;
                }
                "requires" => requires = self.list(Self::reference)?,
                "ensures" => ensures = self.list(Self::reference)?,
                "calls" => calls = self.list(Self::reference)?,
                _ => {
                    return Err(
                        self.at_error(location, format!("Unknown operation field '{field}'"))
                    )
                }
            }
            self.punctuation(';')?;
        }
        let binding =
            binding.ok_or_else(|| self.error("An operation requires a 'bind' declaration"))?;
        Ok(DeclBody::Operation {
            parameters,
            returns,
            purpose,
            binding,
            effects,
            requires,
            ensures,
            calls,
        })
    }

    fn check(&mut self) -> Result<DeclBody, Diagnostic> {
        self.punctuation('{')?;
        let mut command = None;
        let mut cwd = ".".into();
        let mut covers = None;
        let mut timeout = 60;
        let mut seen = HashSet::new();
        while !self.eat('}') {
            let (field, location) = self.ident()?;
            self.singleton(&mut seen, &field, &location)?;
            match field.as_str() {
                "run" => command = Some(self.list(Self::string)?),
                "cwd" => cwd = self.string()?,
                "covers" => covers = Some(self.list(Self::reference)?),
                "timeout" => timeout = self.number()?,
                _ => return Err(self.at_error(location, format!("Unknown check field '{field}'"))),
            }
            self.punctuation(';')?;
        }
        let command = command.ok_or_else(|| self.error("A check requires a 'run' command"))?;
        let covers = covers.ok_or_else(|| self.error("A check requires a 'covers' list"))?;
        Ok(DeclBody::Check {
            command,
            cwd,
            covers,
            timeout,
        })
    }

    fn declaration(&mut self, public: bool) -> Result<Declaration, Diagnostic> {
        let (kind, location) = self.ident()?;
        if !matches!(
            kind.as_str(),
            "type" | "resource" | "requirement" | "boundary" | "operation" | "check"
        ) {
            return Err(self.at_error(location, format!("Unknown declaration '{kind}'")));
        }
        let name = self.ident()?.0;
        let body = match kind.as_str() {
            "type" => {
                self.punctuation(';')?;
                DeclBody::Type
            }
            "resource" => {
                self.punctuation(';')?;
                DeclBody::Resource
            }
            "requirement" => {
                let text = self.string()?;
                self.punctuation(';')?;
                DeclBody::Requirement { text }
            }
            "boundary" => {
                let paths = self.list(Self::string)?;
                self.keyword("allows")?;
                let allows = self.list(Self::reference)?;
                self.punctuation(';')?;
                DeclBody::Boundary { paths, allows }
            }
            "operation" => self.operation()?,
            "check" => self.check()?,
            _ => unreachable!(),
        };
        Ok(Declaration {
            name,
            public,
            location,
            body,
        })
    }

    fn module(&mut self) -> Result<Module, Diagnostic> {
        let mut imports = Vec::new();
        while self.is_keyword("use") {
            let location = self.advance().location;
            let path = self.string()?;
            self.keyword("as")?;
            let alias = self.ident()?.0;
            self.punctuation(';')?;
            imports.push(Import {
                path,
                alias,
                location,
            });
        }
        self.keyword("feature")?;
        let name = self.reference()?.name;
        self.punctuation('{')?;
        let mut purpose = None;
        let mut questions = Vec::new();
        let mut declarations = Vec::new();
        while !self.eat('}') {
            if self.is_keyword("purpose") {
                let location = self.advance().location;
                if purpose.is_some() {
                    return Err(self.at_error(location, "Duplicate feature 'purpose' field"));
                }
                purpose = Some(self.string()?);
                self.punctuation(';')?;
            } else if self.eat_keyword("question") {
                questions.push(self.string()?);
                self.punctuation(';')?;
            } else {
                let public = self.eat_keyword("export");
                declarations.push(self.declaration(public)?);
            }
        }
        if self.token().kind != TokenKind::Eof {
            return Err(self.error("Expected end of file after the feature declaration"));
        }
        Ok(Module {
            path: self.path.clone(),
            name,
            purpose: purpose.unwrap_or_default(),
            imports,
            declarations,
            questions,
        })
    }
}

/// Parse exactly one feature module and preserve source positions on all links.
/// The first lexical or syntax error is returned; no partial module is compiled.
pub fn parse(path: &str, source: &str) -> Result<Module, Vec<Diagnostic>> {
    let lexer = Lexer {
        path,
        source,
        offset: 0,
        line: 1,
        column: 1,
    };
    let tokens = lexer.tokenize().map_err(|error| vec![error])?;
    Parser {
        path: path.into(),
        tokens,
        cursor: 0,
    }
    .module()
    .map_err(|error| vec![error])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(source: &str) -> Module {
        parse("domain/discounts.vitrifi", source).unwrap()
    }

    fn rejects(source: &str, message: &str) -> Diagnostic {
        let errors = parse("feature.vitrifi", source).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains(message), "{}", errors[0].message);
        assert_eq!(errors[0].location.path, "feature.vitrifi");
        errors.into_iter().next().unwrap()
    }

    #[test]
    fn parses_a_feature_with_typed_public_contract_and_relationships() {
        let parsed = module(
            r#"
use "../storage/cart.vitrifi" as Store;
feature Checkout.Discounts {
  purpose "Apply one valid coupon.";
  question "May customers replace an existing coupon?";
  export type Error;
  resource AuditEvents;
  export requirement Owner "Only the owner may change the cart.";
  boundary Domain ["./*.py",] allows [Store.Storage];
  export operation applyCoupon(actor: String, cart: Store.Cart,) -> Result<Store.Cart, Error> {
    purpose "Validate before writing.";
    bind python "./discounts.py" symbol "apply_coupon";
    effects [read Store.Carts, write Store.Carts, emit AuditEvents];
    requires [Owner];
    ensures [Owner];
    calls [Store.load, Store.save,];
  }
  check Behavior {
    run ["python3", "-m", "unittest"];
    cwd "..";
    covers [Owner];
    timeout 30;
  }
}
"#,
        );
        assert_eq!(parsed.name, "Checkout.Discounts");
        assert_eq!(parsed.imports[0].alias, "Store");
        assert_eq!(parsed.imports[0].location.line, 2);
        assert_eq!(parsed.questions.len(), 1);
        assert_eq!(parsed.declarations.len(), 6);
        let operation = &parsed.declarations[4];
        assert!(operation.public);
        let DeclBody::Operation {
            parameters,
            returns,
            binding,
            effects,
            calls,
            ..
        } = &operation.body
        else {
            panic!("operation expected")
        };
        assert_eq!(parameters[1].ty.name, "Store.Cart");
        assert_eq!(returns.name, "Result");
        assert_eq!(returns.arguments.len(), 2);
        assert_eq!(binding.language, "python");
        assert_eq!(binding.symbol, "apply_coupon");
        assert_eq!(effects[1].action, "write");
        assert_eq!(effects[1].resource.name, "Store.Carts");
        assert_eq!(calls[0].name, "Store.load");
        assert_eq!(calls[0].location.line, 16);
    }

    #[test]
    fn minimal_fields_have_explicit_defaults() {
        let parsed = module("feature Empty { operation go() -> Unit { bind ruby \"./a.rb\" symbol \"go\"; } check Test { run []; covers []; } }");
        assert_eq!(parsed.purpose, "");
        assert!(!parsed.declarations[0].public);
        let DeclBody::Operation {
            effects,
            calls,
            requires,
            ensures,
            purpose,
            ..
        } = &parsed.declarations[0].body
        else {
            panic!()
        };
        assert!(
            effects.is_empty() && calls.is_empty() && requires.is_empty() && ensures.is_empty()
        );
        assert_eq!(purpose, &None);
        let DeclBody::Check { cwd, timeout, .. } = &parsed.declarations[1].body else {
            panic!()
        };
        assert_eq!(cwd, ".");
        assert_eq!(*timeout, 60);
    }

    #[test]
    fn comments_and_string_escapes_do_not_become_declarations() {
        let parsed = module(
            r#"// feature Fake {}
/* requirement Nope "ignored"; */
feature Real { purpose "A \"quote\", a \\ slash, a \n newline, and \u00e9"; }
"#,
        );
        assert!(parsed.declarations.is_empty());
        assert_eq!(
            parsed.purpose,
            "A \"quote\", a \\ slash, a \n newline, and é"
        );
    }

    #[test]
    fn references_have_precise_unicode_aware_locations() {
        let parsed = module("feature Name {\n purpose \"é\";\n boundary App [\"./é/*\"] allows [Store.Database];\n}");
        let DeclBody::Boundary { allows, .. } = &parsed.declarations[0].body else {
            panic!()
        };
        assert_eq!(allows[0].location.line, 3);
        assert_eq!(allows[0].location.column, 33);
    }

    #[test]
    fn rejects_duplicate_singleton_fields_in_each_scope() {
        rejects(
            "feature X { purpose \"x\"; purpose \"y\"; }",
            "Duplicate feature 'purpose'",
        );
        rejects("feature X { operation go() -> Unit { bind python \"a.py\" symbol \"go\"; effects []; effects []; } }", "Duplicate 'effects'");
        rejects(
            "feature X { check Test { run []; covers []; timeout 1; timeout 2; } }",
            "Duplicate 'timeout'",
        );
    }

    #[test]
    fn unknown_declarations_and_fields_are_errors() {
        rejects("feature X { typo App; }", "Unknown declaration 'typo'");
        rejects(
            "feature X { operation go() -> Unit { binds python \"a.py\"; } }",
            "Unknown operation field 'binds'",
        );
        rejects(
            "feature X { check Test { proves [Owner]; } }",
            "Unknown check field 'proves'",
        );
    }

    #[test]
    fn mandatory_binding_and_check_fields_are_enforced() {
        rejects(
            "feature X { operation go() -> Unit {} }",
            "requires a 'bind'",
        );
        rejects(
            "feature X { check Test { covers []; } }",
            "requires a 'run'",
        );
        rejects(
            "feature X { check Test { run []; } }",
            "requires a 'covers'",
        );
    }

    #[test]
    fn unknown_effect_verbs_are_rejected() {
        rejects("feature X { operation go() -> Unit { bind python \"a.py\" symbol \"go\"; effects [delete Cart]; } }", "Unknown effect action 'delete'");
    }

    #[test]
    fn invalid_and_unterminated_strings_are_errors() {
        rejects(
            r#"feature X { purpose "bad \q"; }"#,
            "Invalid string literal",
        );
        rejects("feature X { purpose \"broken", "Unterminated string");
        rejects("feature X { purpose \"a\nb\"; }", "unescaped newline");
        rejects(
            r#"feature X { purpose "\uD800"; }"#,
            "Invalid string literal",
        );
    }

    #[test]
    fn unterminated_comments_and_unknown_characters_are_errors() {
        rejects("feature X {} /* open", "Unterminated block comment");
        let error = rejects("feature X {\n @\n}", "Unexpected character '@'");
        assert_eq!(error.location.line, 2);
        assert_eq!(error.location.column, 2);
    }

    #[test]
    fn incomplete_input_and_trailing_junk_are_rejected() {
        rejects("", "Expected 'feature'");
        rejects("feature X {", "Expected an identifier");
        rejects("feature X {} feature Y {}", "Expected end of file");
        rejects("feature X { type A }", "Expected ';'");
        rejects(
            "feature X { use \"a.vitrifi\" as A; }",
            "Unknown declaration 'use'",
        );
    }

    #[test]
    fn nested_generics_are_preserved_and_depth_is_bounded() {
        let parsed = module("feature X { operation go(x: List<Option<String>>) -> Result<Unit, Error> { bind java \"A.java\" symbol \"A.go\"; } }");
        let DeclBody::Operation { parameters, .. } = &parsed.declarations[0].body else {
            panic!()
        };
        assert_eq!(parameters[0].ty.arguments[0].arguments[0].name, "String");
        let source = format!("feature X {{ operation go(x: {}String{}) -> Unit {{ bind python \"a.py\" symbol \"go\"; }} }}", "List<".repeat(70), ">".repeat(70));
        rejects(&source, "Type nesting exceeds");
    }

    #[test]
    fn identifiers_are_case_sensitive_and_bad_timeouts_are_rejected() {
        let parsed = module("feature Checkout.API { type Cart; type cart; }");
        assert_ne!(parsed.declarations[0].name, parsed.declarations[1].name);
        rejects(
            "feature X { check Test { run []; covers []; timeout -1; } }",
            "Unexpected character '-'",
        );
        rejects(
            "feature X { check Test { run []; covers []; timeout 18446744073709551616; } }",
            "Integer exceeds",
        );
    }
}
