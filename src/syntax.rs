//! Module implementing custom syntax for [`Engine`].

use crate::ast::Expr;
use crate::engine::EvalContext;
use crate::fn_native::SendSync;
use crate::stdlib::{boxed::Box, format, string::ToString};
use crate::token::{is_valid_identifier, Token};
use crate::{
    Engine, Identifier, ImmutableString, LexError, ParseError, Position, RhaiResult, Shared,
    StaticVec,
};

pub const MARKER_EXPR: &str = "$expr$";
pub const MARKER_BLOCK: &str = "$block$";
pub const MARKER_IDENT: &str = "$ident$";

/// A general expression evaluation trait object.
#[cfg(not(feature = "sync"))]
pub type FnCustomSyntaxEval = dyn Fn(&mut EvalContext, &[Expression]) -> RhaiResult;
/// A general expression evaluation trait object.
#[cfg(feature = "sync")]
pub type FnCustomSyntaxEval = dyn Fn(&mut EvalContext, &[Expression]) -> RhaiResult + Send + Sync;

/// A general expression parsing trait object.
#[cfg(not(feature = "sync"))]
pub type FnCustomSyntaxParse =
    dyn Fn(&[ImmutableString], &str) -> Result<Option<ImmutableString>, ParseError>;
/// A general expression parsing trait object.
#[cfg(feature = "sync")]
pub type FnCustomSyntaxParse =
    dyn Fn(&[ImmutableString], &str) -> Result<Option<ImmutableString>, ParseError> + Send + Sync;

/// An expression sub-tree in an [`AST`][crate::AST].
#[derive(Debug, Clone)]
pub struct Expression<'a>(&'a Expr);

impl<'a> From<&'a Expr> for Expression<'a> {
    #[inline(always)]
    fn from(expr: &'a Expr) -> Self {
        Self(expr)
    }
}

impl Expression<'_> {
    /// If this expression is a variable name, return it.  Otherwise [`None`].
    #[inline(always)]
    pub fn get_variable_name(&self) -> Option<&str> {
        self.0.get_variable_access(true)
    }
    /// Get the expression.
    #[inline(always)]
    pub(crate) fn expr(&self) -> &Expr {
        &self.0
    }
    /// Get the position of this expression.
    #[inline(always)]
    pub fn position(&self) -> Position {
        self.0.position()
    }
}

impl EvalContext<'_, '_, '_, '_, '_, '_, '_> {
    /// Evaluate an [expression tree][Expression].
    ///
    /// # WARNING - Low Level API
    ///
    /// This function is very low level.  It evaluates an expression from an [`AST`][crate::AST].
    #[inline(always)]
    pub fn eval_expression_tree(&mut self, expr: &Expression) -> RhaiResult {
        self.engine.eval_expr(
            self.scope,
            self.mods,
            self.state,
            self.lib,
            self.this_ptr,
            expr.expr(),
            self.level,
        )
    }
}

/// Definition of a custom syntax definition.
pub struct CustomSyntax {
    /// A parsing function to return the next keyword in a custom syntax based on the
    /// keywords parsed so far.
    pub parse: Box<FnCustomSyntaxParse>,
    /// Custom syntax implementation function.
    pub func: Shared<FnCustomSyntaxEval>,
    /// Delta number of variables in the scope.
    pub scope_delta: isize,
}

impl Engine {
    /// Register a custom syntax with the [`Engine`].
    ///
    /// * `keywords` holds a slice of strings that define the custom syntax.  
    /// * `new_vars` is the number of new variables declared by this custom syntax, or the number of variables removed (if negative).
    /// * `func` is the implementation function.
    ///
    /// # Notes
    ///
    /// If `new_vars` is positive, then a number of new variables are expected to be pushed into the
    /// current [`Scope`][crate::Scope].
    ///
    /// If `new_vars` is negative, then it is expected that only the top `new_vars` variables in the
    /// current [`Scope`][crate::Scope] will be _popped_.  Do not randomly remove variables.
    pub fn register_custom_syntax<S: AsRef<str> + Into<Identifier>>(
        &mut self,
        keywords: &[S],
        new_vars: isize,
        func: impl Fn(&mut EvalContext, &[Expression]) -> RhaiResult + SendSync + 'static,
    ) -> Result<&mut Self, ParseError> {
        let keywords = keywords.as_ref();

        let mut segments: StaticVec<ImmutableString> = Default::default();

        for s in keywords {
            let s = s.as_ref().trim();

            // skip empty keywords
            if s.is_empty() {
                continue;
            }

            let token = Token::lookup_from_syntax(s);

            let seg = match s {
                // Markers not in first position
                MARKER_IDENT | MARKER_EXPR | MARKER_BLOCK if !segments.is_empty() => s.into(),
                // Standard or reserved keyword/symbol not in first position
                s if !segments.is_empty() && token.is_some() => {
                    // Make it a custom keyword/symbol if it is disabled or reserved
                    if (self.disabled_symbols.contains(s)
                        || matches!(token, Some(Token::Reserved(_))))
                        && !self.custom_keywords.contains_key(s)
                    {
                        self.custom_keywords.insert(s.into(), None);
                    }
                    s.into()
                }
                // Standard keyword in first position
                s if segments.is_empty()
                    && token
                        .as_ref()
                        .map_or(false, |v| v.is_keyword() || v.is_reserved()) =>
                {
                    return Err(LexError::ImproperSymbol(
                        s.to_string(),
                        format!(
                            "Improper symbol for custom syntax at position #{}: '{}'",
                            segments.len() + 1,
                            s
                        ),
                    )
                    .into_err(Position::NONE)
                    .into());
                }
                // Identifier in first position
                s if segments.is_empty() && is_valid_identifier(s.chars()) => {
                    // Make it a custom keyword/symbol if it is disabled or reserved
                    if (self.disabled_symbols.contains(s)
                        || matches!(token, Some(Token::Reserved(_))))
                        && !self.custom_keywords.contains_key(s)
                    {
                        self.custom_keywords.insert(s.into(), None);
                    }
                    s.into()
                }
                // Anything else is an error
                _ => {
                    return Err(LexError::ImproperSymbol(
                        s.to_string(),
                        format!(
                            "Improper symbol for custom syntax at position #{}: '{}'",
                            segments.len() + 1,
                            s
                        ),
                    )
                    .into_err(Position::NONE)
                    .into());
                }
            };

            segments.push(seg);
        }

        // If the syntax has no keywords, just ignore the registration
        if segments.is_empty() {
            return Ok(self);
        }

        // The first keyword is the discriminator
        let key = segments[0].clone();

        self.register_custom_syntax_raw(
            key,
            // Construct the parsing function
            move |stream, _| {
                if stream.len() >= segments.len() {
                    Ok(None)
                } else {
                    Ok(Some(segments[stream.len()].clone()))
                }
            },
            new_vars,
            func,
        );

        Ok(self)
    }
    /// Register a custom syntax with the [`Engine`].
    ///
    /// # WARNING - Low Level API
    ///
    /// This function is very low level.
    ///
    /// * `new_vars` is the number of new variables declared by this custom syntax, or the number of variables removed (if negative).
    /// * `parse` is the parsing function.
    /// * `func` is the implementation function.
    ///
    /// All custom keywords must be manually registered via [`Engine::register_custom_operator`].
    /// Otherwise, custom keywords won't be recognized.
    pub fn register_custom_syntax_raw(
        &mut self,
        key: impl Into<Identifier>,
        parse: impl Fn(&[ImmutableString], &str) -> Result<Option<ImmutableString>, ParseError>
            + SendSync
            + 'static,
        new_vars: isize,
        func: impl Fn(&mut EvalContext, &[Expression]) -> RhaiResult + SendSync + 'static,
    ) -> &mut Self {
        let syntax = CustomSyntax {
            parse: Box::new(parse),
            func: (Box::new(func) as Box<FnCustomSyntaxEval>).into(),
            scope_delta: new_vars,
        };

        self.custom_syntax.insert(key.into(), syntax);
        self
    }
}
