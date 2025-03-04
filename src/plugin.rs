//! Module defining macros for developing _plugins_.

pub use crate::fn_native::{CallableFunction, FnCallArgs};
pub use crate::stdlib::{any::TypeId, boxed::Box, format, mem, string::ToString};
pub use crate::{
    Dynamic, Engine, EvalAltResult, FnAccess, FnNamespace, ImmutableString, Module,
    NativeCallContext, Position,
};
pub type RhaiResult = Result<Dynamic, Box<EvalAltResult>>;

#[cfg(not(features = "no_module"))]
pub use rhai_codegen::*;
#[cfg(features = "no_module")]
pub use rhai_codegen::{export_fn, register_exported_fn};

/// Trait implemented by a _plugin function_.
///
/// This trait should not be used directly.
/// Use the `#[export_module]` and `#[export_fn]` procedural attributes instead.
pub trait PluginFunction {
    /// Call the plugin function with the arguments provided.
    fn call(&self, context: NativeCallContext, args: &mut FnCallArgs) -> RhaiResult;

    /// Is this plugin function a method?
    fn is_method_call(&self) -> bool;

    /// Is this plugin function variadic?
    fn is_variadic(&self) -> bool;
}
