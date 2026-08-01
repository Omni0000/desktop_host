//! Binding specification and metadata types.
//!
//! A [`Binding`] defines an abstract contract specifying what plugins must implement
//! (via plugs) or what they could depend on (via sockets). It bundles one or more WIT
//! [`Interface`]s under a single identifier.

use std::collections::HashMap ;
use std::sync::Arc ;
use futures::lock::Mutex ;
use wasmtime::component::{ Linker, Val };

use crate::{ Interface, PluginContext };
use crate::cardinality::{ Any, AtLeastOne, AtMostOne, Cardinality, ExactlyOne };
use crate::plugin_instance::{ PluginInstanceAsync, PluginInstanceSync };

pub(crate) type ImportAsyncness = HashMap<String, ImportMetadata>;

pub(crate) struct ImportMetadata {
	pub(crate) linker_name: String,
	pub(crate) functions: HashMap<String, bool>,
}



type PluginSockets<PluginId, Plugins, Instance> =
	<Plugins as Cardinality<PluginId, Instance>>::Rebind<Arc<Mutex<Instance>>> ;

type DispatchResults<PluginId, Plugins, Instance> =
	<PluginSockets<PluginId, Plugins, Instance> as Cardinality<PluginId, Arc<Mutex<Instance>>>>::Rebind<
		Result<wasmtime::component::Val, crate::DispatchError>
	>;

type DispatchVals<PluginId, Plugins, Instance> =
	<PluginSockets<PluginId, Plugins, Instance> as Cardinality<PluginId, Arc<Mutex<Instance>>>>::Rebind<
		wasmtime::component::Val
	>;

struct BindingData<PluginId, Plugins, Instance>
where
	PluginId: std::hash::Hash + Eq + Clone + Send + Sync + 'static,
	Instance: Send + 'static,
	Plugins: Cardinality<PluginId, Instance>,
	PluginSockets<PluginId, Plugins, Instance>: Send + Sync,
{
	package_name: String,
	interfaces: HashMap<String, Interface>,
	plugins: PluginSockets<PluginId, Plugins, Instance>,
}

/// An abstract contract specifying what plugins must implement (via plugs) or what
/// they could depend on (via sockets). It bundles one or more WIT [`Interface`]s
/// under a single package name.
///
/// `Binding` is a handle to shared state. Cloning a `Binding` creates another handle
/// to the same underlying binding, enabling shared dependencies where multiple
/// plugins depend on the same binding.
///
/// ```
/// # use std::collections::{ HashMap, HashSet };
/// # use wasm_link::{ Binding, Interface, Function, FunctionKind, ReturnKind, Plugin, Engine, Component, Linker, ResourceTable };
/// # use wasm_link::cardinality::ExactlyOne ;
/// # struct Ctx { resource_table: ResourceTable }
/// # impl wasm_link::PluginContext for Ctx {
/// # 	fn resource_table( &mut self ) -> &mut ResourceTable { &mut self.resource_table }
/// # }
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// # let engine = Engine::default();
/// # let linker = Linker::new( &engine );
/// # let plugin = Plugin::new( Component::new( &engine, "(component)" )?, Ctx { resource_table: ResourceTable::new() }).instantiate( &engine, &linker )?;
/// let binding: Binding<String, Ctx> = Binding::new(
/// 	"my:package",
/// 	HashMap::from([
/// 		( "api".to_string(), Interface::new(
/// 			HashMap::from([( "get-value".into(), Function::new(
/// 				FunctionKind::Freestanding,
/// 				ReturnKind::MayContainResources,
/// 			))]),
/// 			HashSet::from([ "my-resource".to_string() ]),
/// 		)),
/// 	]),
/// 	ExactlyOne( "my-plugin".to_string(), plugin ),
/// );
///
/// // Clone for shared dependencies - both refer to the same binding
/// let binding_clone = binding.clone();
/// # let binding_any_clone = binding.into_any().clone();
/// # Ok(())
/// # }
/// ```
///
/// # Type Parameters
/// - `PluginId`: Unique identifier type for plugins (e.g., `String`, `UUID`)
/// - `Ctx`: Context stored in each plugin's Wasmtime store
/// - `Plugins`: Cardinality wrapper containing the plugin instances
/// - `Instance`: [`PluginInstanceSync`] or [`PluginInstanceAsync`]
pub struct Binding<PluginId, Ctx, Plugins = ExactlyOne<PluginId, PluginInstanceSync<Ctx>>, Instance = PluginInstanceSync<Ctx>>(
	Arc<BindingData<PluginId, Plugins, Instance>>,
	std::marker::PhantomData<fn() -> Ctx>,
)
where
	PluginId: std::hash::Hash + Eq + Clone + Send + Sync + 'static,
	Ctx: PluginContext + 'static,
	Instance: Send + 'static,
	Plugins: Cardinality<PluginId, Instance> + 'static,
	PluginSockets<PluginId, Plugins, Instance>: Send + Sync;

impl<PluginId, Ctx, Plugins, Instance> Clone for Binding<PluginId, Ctx, Plugins, Instance>
where
	PluginId: std::hash::Hash + Eq + Clone + Send + Sync + 'static,
	Ctx: PluginContext + 'static,
	Instance: Send + 'static,
	Plugins: Cardinality<PluginId, Instance> + 'static,
	PluginSockets<PluginId, Plugins, Instance>: Send + Sync,
{
	/// Creates another handle to the same underlying binding, enabling shared dependencies where
	/// multiple plugins depend on the same binding.
	fn clone( &self ) -> Self {
		Self( Arc::clone( &self.0 ), std::marker::PhantomData )
	}
}

impl<PluginId, Ctx, Plugins, Instance> std::fmt::Debug for Binding<PluginId, Ctx, Plugins, Instance>
where
	PluginId: std::hash::Hash + Eq + Clone + Send + Sync + std::fmt::Debug + 'static,
	Ctx: PluginContext + std::fmt::Debug + 'static,
	Instance: Send + 'static,
	Plugins: Cardinality<PluginId, Instance> + 'static,
	PluginSockets<PluginId, Plugins, Instance>: Send + Sync + std::fmt::Debug,
{
	fn fmt( &self, f: &mut std::fmt::Formatter<'_> ) -> std::fmt::Result {
		f.debug_struct( "Binding" )
			.field( "package_name", &self.0.package_name )
			.field( "interfaces", &self.0.interfaces )
			.field( "plugins", &self.0.plugins )
			.finish()
	}
}

impl<PluginId, Ctx, Plugins, Instance> Binding<PluginId, Ctx, Plugins, Instance>
where
	PluginId: std::hash::Hash + Eq + Clone + Send + Sync + 'static,
	Ctx: PluginContext + 'static,
	Instance: Send + 'static,
	Plugins: Cardinality<PluginId, Instance> + 'static,
	PluginSockets<PluginId, Plugins, Instance>: Cardinality<PluginId, Arc<Mutex<Instance>>> + Send + Sync,
{

	/// Creates a new binding specification.
	pub fn new(
		package_name: impl Into<String>,
		interfaces: HashMap<String, Interface>,
		plugins: Plugins,
	) -> Self {
		let package_name = package_name.into();
		Self( Arc::new( BindingData {
			package_name,
			interfaces,
			plugins: plugins.map_mut(| plugin | Arc::new( Mutex::new( plugin ))),
		}), std::marker::PhantomData )
	}

	pub(crate) fn plugins( &self ) -> &PluginSockets<PluginId, Plugins, Instance> {
		&self.0.plugins
	}

	pub(crate) fn package_name( &self ) -> &str { &self.0.package_name }

}

impl<PluginId, Ctx, Plugins> Binding<PluginId, Ctx, Plugins, PluginInstanceSync<Ctx>>
where
	PluginId: std::hash::Hash + Eq + Clone + Send + Sync + 'static,
	Ctx: PluginContext + 'static,
	Plugins: Cardinality<PluginId, PluginInstanceSync<Ctx>> + 'static,
	PluginSockets<PluginId, Plugins, PluginInstanceSync<Ctx>>: Cardinality<PluginId, Arc<Mutex<PluginInstanceSync<Ctx>>>> + Send + Sync,
{

	pub(crate) fn add_to_linker( binding: &Binding<PluginId, Ctx, Plugins>, linker: &mut Linker<Ctx> ) -> Result<(), wasmtime::Error>
	where
		PluginId: Into<Val>,
		DispatchVals<PluginId, Plugins, PluginInstanceSync<Ctx>>: Into<Val>,
	{
		binding.0.interfaces.iter().try_for_each(|( name, interface )| {
			let interface_ident = format!( "{}/{}", binding.0.package_name, name );
			interface.add_to_linker( linker, &interface_ident, name, binding )
		})
	}

	pub(crate) fn add_sync_to_linker_async(
		binding: &Binding<PluginId, Ctx, Plugins>,
		linker: &mut Linker<Ctx>,
		imports: &ImportAsyncness,
	) -> Result<(), wasmtime::Error>
	where
		PluginId: Into<Val>,
		DispatchVals<PluginId, Plugins, PluginInstanceSync<Ctx>>: Into<Val> + Send,
	{
		binding.0.interfaces.iter().try_for_each(|( name, interface )| {
			let interface_ident = format!( "{}/{}", binding.0.package_name, name );
			let import = imports.get( &interface_ident );
			let linker_name = import.map_or( interface_ident.as_str(), | import | import.linker_name.as_str() );
			interface.add_to_linker_async_sync(
				linker,
				linker_name,
				name,
				binding,
				import.map(| import | &import.functions ),
			)
		})
	}

	/// Dispatches a function call to all plugins implementing this binding.
	///
	/// This is used for external dispatch (calling into the plugin graph from outside).
	/// The result is wrapped in a type matching the binding's cardinality.
	///
	/// # Arguments
	/// * `interface_name` - The interface name within this binding (e.g., "example")
	/// * `function_name` - The function name within the interface (e.g., "get-value")
	/// * `args` - Arguments to pass to the function
	///
	/// # Returns
	/// A cardinality wrapper containing `Result<Val, DispatchError>` for each plugin.
	/// For [`ReturnKind::Void`]( crate::ReturnKind::Void ), the value is an empty tuple
	/// (`Val::Option( None )`) placeholder.
	///
	/// # Errors
	/// Returns an error if the interface or function is not found in this binding.
	pub fn dispatch(
		&self,
		interface_name: &str,
		function_name: &str,
		args: &[wasmtime::component::Val],
	) -> Result<DispatchResults<PluginId, Plugins, PluginInstanceSync<Ctx>>, crate::DispatchError> {

		let interface = self.0.interfaces.get( interface_name )
			.ok_or_else(|| crate::DispatchError::InvalidInterfacePath( format!( "{}/{}", self.0.package_name, interface_name )))?;

		let function = interface.function( function_name )
			.ok_or_else(|| crate::DispatchError::InvalidFunction( function_name.to_string() ))?;

		Ok( self.0.plugins.map(| _, plugin | plugin
			.try_lock().ok_or( crate::DispatchError::LockRejected )
			.and_then(| mut lock | lock.dispatch(
				&self.0.package_name,
				interface_name,
				function_name,
				function,
				args,
			))
		))

	}


}

impl<PluginId, Ctx, Plugins> Binding<PluginId, Ctx, Plugins, PluginInstanceAsync<Ctx>>
where
	PluginId: std::hash::Hash + Eq + Clone + Send + Sync + 'static,
	Ctx: PluginContext + 'static,
	Plugins: Cardinality<PluginId, PluginInstanceAsync<Ctx>> + 'static,
	PluginSockets<PluginId, Plugins, PluginInstanceAsync<Ctx>>: Cardinality<PluginId, Arc<Mutex<PluginInstanceAsync<Ctx>>>> + Send + Sync,
{
	pub(crate) fn add_to_linker_async(
		binding: &Self,
		linker: &mut Linker<Ctx>,
		active_calls: &crate::async_scheduler::ActiveCalls<Ctx>,
		imports: &ImportAsyncness,
	) -> Result<(), wasmtime::Error>
	where
		PluginId: Into<Val>,
		DispatchVals<PluginId, Plugins, PluginInstanceAsync<Ctx>>: Into<Val> + Send,
	{
		binding.0.interfaces.iter().try_for_each(|( name, interface )| {
			let interface_ident = format!( "{}/{}", binding.0.package_name, name );
			let import = imports.get( &interface_ident );
			let linker_name = import.map_or( interface_ident.as_str(), | import | import.linker_name.as_str() );
			interface.add_to_linker_async(
				linker,
				linker_name,
				name,
				binding,
				&crate::async_scheduler::LinkDispatchContext::new( active_calls ),
				import.map(| import | &import.functions ),
			)
		})
	}

	/// Asynchronously dispatches a function call to all plugins implementing this binding.
	///
	/// This method waits for a busy plugin instead of returning [`DispatchError::LockRejected`](crate::DispatchError::LockRejected).
	/// It is required for instances created by [`Plugin::instantiate_async`](crate::Plugin::instantiate_async)
	/// or [`Plugin::link_async`](crate::Plugin::link_async).
	/// Calls made by the resulting plugin graph share one cooperative dispatch session.
	/// Independent dispatches to the same plugin are served serially.
	///
	/// # Example
	///
	/// ```
	/// # use std::collections::{ HashMap, HashSet };
	/// # use wasm_link::{ Binding, Component, Engine, Function, FunctionKind, Interface, Linker, Plugin, PluginContext, ResourceTable, ReturnKind, Val };
	/// # use wasm_link::cardinality::ExactlyOne;
	/// # struct Context { table: ResourceTable }
	/// # impl PluginContext for Context { fn resource_table( &mut self ) -> &mut ResourceTable { &mut self.table } }
	/// # fn main() -> Result<(), Box<dyn std::error::Error>> { futures::executor::block_on( async {
	/// # let engine = Engine::default();
	/// # let linker = Linker::new( &engine );
	/// # let component = Component::new( &engine, r#"(component
	/// # 	(core module $m (func (export "get") (result i32) i32.const 42))
	/// # 	(core instance $i (instantiate $m))
	/// # 	(func $get (result u32) (canon lift (core func $i "get")))
	/// # 	(instance $root (export "get" (func $get)))
	/// # 	(export "example:plugin/root" (instance $root))
	/// # )"# )?;
	/// # let plugin = Plugin::new( component, Context { table: ResourceTable::new() })
	/// # 	.instantiate_async( &engine, &linker ).await?;
	/// # let binding = Binding::new(
	/// # 	"example:plugin",
	/// # 	HashMap::from([( "root".to_string(), Interface::new(
	/// # 		HashMap::from([( "get".to_string(), Function::new( FunctionKind::Freestanding, ReturnKind::MayContainResources ))]),
	/// # 		HashSet::new(),
	/// # 	))]),
	/// # 	ExactlyOne( "plugin".to_string(), plugin ),
	/// # );
	/// let result = binding.dispatch( "root", "get", &[] ).await?;
	/// assert!( matches!( result, ExactlyOne( _, Ok( Val::U32( 42 )))));
	/// # Ok(()) }) }
	/// ```
	///
	/// # Errors
	/// Returns an error if the interface or function is not found in this binding.
	pub async fn dispatch(
		&self,
		interface_name: &str,
		function_name: &str,
		args: &[wasmtime::component::Val],
	) -> Result<DispatchResults<PluginId, Plugins, PluginInstanceAsync<Ctx>>, crate::DispatchError>
	where
		PluginId: Into<Val>,
		DispatchResults<PluginId, Plugins, PluginInstanceAsync<Ctx>>: Send,
	{
		let binding = self.clone();
		let interface_name = interface_name.to_string();
		let function_name = function_name.to_string();
		let args = args.to_vec();
		crate::async_scheduler::run( move | scheduler | async move {
			binding.dispatch_in_scheduler( &scheduler, &interface_name, &function_name, &args ).await
		}).await
	}

	async fn dispatch_in_scheduler(
		&self,
		scheduler: &crate::async_scheduler::AsyncScheduler<Ctx>,
		interface_name: &str,
		function_name: &str,
		args: &[wasmtime::component::Val],
	) -> Result<DispatchResults<PluginId, Plugins, PluginInstanceAsync<Ctx>>, crate::DispatchError>
	where
		PluginId: Into<Val>,
		DispatchResults<PluginId, Plugins, PluginInstanceAsync<Ctx>>: Send,
	{
		let interface = self.0.interfaces.get( interface_name )
			.ok_or_else(|| crate::DispatchError::InvalidInterfacePath( format!( "{}/{}", self.0.package_name, interface_name )))?;
		let function = interface.function( function_name )
			.ok_or_else(|| crate::DispatchError::InvalidFunction( function_name.to_string() ))?;
		let package_name = self.0.package_name.clone();
		let interface_name = interface_name.to_string();
		let function_name = function_name.to_string();
		let function = function.clone();
		let args = args.to_vec();
		let caller = scheduler.origin();
		let path = crate::async_scheduler::ExecutionPathId::ROOT;

		Ok( self.0.plugins.map_async(| _, plugin | {
			let package_name = package_name.clone();
			let interface_name = interface_name.clone();
			let function_name = function_name.clone();
			let function = function.clone();
			let args = args.clone();
			let scheduler = scheduler.clone();
			async move {
				let instance = plugin.lock().await.handle();
				instance.dispatch_async(
					crate::async_scheduler::DispatchContext::new( &scheduler, caller, path ),
					&package_name,
					&interface_name,
					&function_name,
					&function,
					&args,
				).await
			}
		}).await )
	}

}

/// Type-erased binding wrapper for heterogeneous socket lists.
///
/// Use when a plugin's sockets include bindings with different cardinalities.
#[derive( Debug )]
pub enum BindingAny<PluginId, Ctx, Instance = PluginInstanceSync<Ctx>>
where
	PluginId: std::hash::Hash + Eq + Clone + Send + Sync + 'static,
	Ctx: PluginContext + 'static,
	Instance: Send + 'static,
{
	/// Exactly one plugin implementation.
	ExactlyOne( Binding<PluginId, Ctx, ExactlyOne<PluginId, Instance>, Instance> ),
	/// Zero or one plugin implementation.
	AtMostOne( Binding<PluginId, Ctx, AtMostOne<PluginId, Instance>, Instance> ),
	/// One or more plugin implementations.
	AtLeastOne( Binding<PluginId, Ctx, AtLeastOne<PluginId, Instance>, Instance> ),
	/// Zero or more plugin implementations.
	Any( Binding<PluginId, Ctx, Any<PluginId, Instance>, Instance> ),
}

impl<PluginId, Ctx, Instance> BindingAny<PluginId, Ctx, Instance>
where
	PluginId: std::hash::Hash + Eq + Clone + Send + Sync + 'static,
	Ctx: PluginContext + 'static,
	Instance: Send + 'static,
{
}

impl<PluginId, Ctx> BindingAny<PluginId, Ctx, PluginInstanceSync<Ctx>>
where
	PluginId: std::hash::Hash + Eq + Clone + Send + Sync + Into<Val> + 'static,
	Ctx: PluginContext + 'static,
{
	pub(crate) fn add_to_linker( &self, linker: &mut Linker<Ctx> ) -> Result<(), wasmtime::Error> {
		match self {
			Self::ExactlyOne( binding ) => Binding::add_to_linker( binding, linker ),
			Self::AtMostOne( binding ) => Binding::add_to_linker( binding, linker ),
			Self::AtLeastOne( binding ) => Binding::add_to_linker( binding, linker ),
			Self::Any( binding ) => Binding::add_to_linker( binding, linker ),
		}
	}

	pub(crate) fn add_to_linker_async(
		&self,
		linker: &mut Linker<Ctx>,
		imports: &ImportAsyncness,
	) -> Result<(), wasmtime::Error> {
		match self {
			Self::ExactlyOne( binding ) => Binding::add_sync_to_linker_async( binding, linker, imports ),
			Self::AtMostOne( binding ) => Binding::add_sync_to_linker_async( binding, linker, imports ),
			Self::AtLeastOne( binding ) => Binding::add_sync_to_linker_async( binding, linker, imports ),
			Self::Any( binding ) => Binding::add_sync_to_linker_async( binding, linker, imports ),
		}
	}

}

/// Type-erased socket accepted by [`Plugin::link_async`](crate::Plugin::link_async).
///
/// This can hold either a synchronous binding or an asynchronously dispatchable
/// binding, allowing both kinds to appear in one socket list.
#[derive( Debug )]
pub struct BindingAnyAsync<PluginId, Ctx>( BindingAnyAsyncKind<PluginId, Ctx> )
where
	PluginId: std::hash::Hash + Eq + Clone + Send + Sync + 'static,
	Ctx: PluginContext + 'static;

#[derive( Debug )]
enum BindingAnyAsyncKind<PluginId, Ctx>
where
	PluginId: std::hash::Hash + Eq + Clone + Send + Sync + 'static,
	Ctx: PluginContext + 'static,
{
	Sync( BindingAny<PluginId, Ctx, PluginInstanceSync<Ctx>> ),
	Async( BindingAny<PluginId, Ctx, PluginInstanceAsync<Ctx>> ),
}

impl<PluginId, Ctx> BindingAnyAsync<PluginId, Ctx>
where
	PluginId: std::hash::Hash + Eq + Clone + Send + Sync + Into<Val> + 'static,
	Ctx: PluginContext + 'static,
{
	pub(crate) fn add_to_linker(
		&self,
		linker: &mut Linker<Ctx>,
		active_calls: &crate::async_scheduler::ActiveCalls<Ctx>,
		imports: &ImportAsyncness,
	) -> Result<(), wasmtime::Error> {
		match &self.0 {
			BindingAnyAsyncKind::Sync( binding ) => binding.add_to_linker_async( linker, imports ),
			BindingAnyAsyncKind::Async( binding ) => binding.add_to_linker_async( linker, active_calls, imports ),
		}
	}
}

impl<PluginId, Ctx, Plugins> From<Binding<PluginId, Ctx, Plugins, PluginInstanceSync<Ctx>>> for BindingAnyAsync<PluginId, Ctx>
where
	PluginId: std::hash::Hash + Eq + Clone + Send + Sync + 'static,
	Ctx: PluginContext + 'static,
	Plugins: Cardinality<PluginId, PluginInstanceSync<Ctx>> + 'static,
	PluginSockets<PluginId, Plugins, PluginInstanceSync<Ctx>>: Send + Sync,
	BindingAny<PluginId, Ctx, PluginInstanceSync<Ctx>>: From<Binding<PluginId, Ctx, Plugins, PluginInstanceSync<Ctx>>>,
{
	fn from( binding: Binding<PluginId, Ctx, Plugins, PluginInstanceSync<Ctx>> ) -> Self {
		Self( BindingAnyAsyncKind::Sync( binding.into() ))
	}
}

impl<PluginId, Ctx, Plugins> From<Binding<PluginId, Ctx, Plugins, PluginInstanceAsync<Ctx>>> for BindingAnyAsync<PluginId, Ctx>
where
	PluginId: std::hash::Hash + Eq + Clone + Send + Sync + 'static,
	Ctx: PluginContext + 'static,
	Plugins: Cardinality<PluginId, PluginInstanceAsync<Ctx>> + 'static,
	PluginSockets<PluginId, Plugins, PluginInstanceAsync<Ctx>>: Send + Sync,
	BindingAny<PluginId, Ctx, PluginInstanceAsync<Ctx>>: From<Binding<PluginId, Ctx, Plugins, PluginInstanceAsync<Ctx>>>,
{
	fn from( binding: Binding<PluginId, Ctx, Plugins, PluginInstanceAsync<Ctx>> ) -> Self {
		Self( BindingAnyAsyncKind::Async( binding.into() ))
	}
}

impl<PluginId, Ctx> From<BindingAny<PluginId, Ctx, PluginInstanceSync<Ctx>>> for BindingAnyAsync<PluginId, Ctx>
where
	PluginId: std::hash::Hash + Eq + Clone + Send + Sync + 'static,
	Ctx: PluginContext + 'static,
{
	fn from( binding: BindingAny<PluginId, Ctx, PluginInstanceSync<Ctx>> ) -> Self {
		Self( BindingAnyAsyncKind::Sync( binding ))
	}
}

impl<PluginId, Ctx> From<BindingAny<PluginId, Ctx, PluginInstanceAsync<Ctx>>> for BindingAnyAsync<PluginId, Ctx>
where
	PluginId: std::hash::Hash + Eq + Clone + Send + Sync + 'static,
	Ctx: PluginContext + 'static,
{
	fn from( binding: BindingAny<PluginId, Ctx, PluginInstanceAsync<Ctx>> ) -> Self {
		Self( BindingAnyAsyncKind::Async( binding ))
	}
}

impl<PluginId, Ctx> BindingAny<PluginId, Ctx, PluginInstanceAsync<Ctx>>
where
	PluginId: std::hash::Hash + Eq + Clone + Send + Sync + Into<Val> + 'static,
	Ctx: PluginContext + 'static,
{
	pub(crate) fn add_to_linker_async(
		&self,
		linker: &mut Linker<Ctx>,
		active_calls: &crate::async_scheduler::ActiveCalls<Ctx>,
		imports: &ImportAsyncness,
	) -> Result<(), wasmtime::Error> {
		match self {
			Self::ExactlyOne( binding ) => Binding::add_to_linker_async( binding, linker, active_calls, imports ),
			Self::AtMostOne( binding ) => Binding::add_to_linker_async( binding, linker, active_calls, imports ),
			Self::AtLeastOne( binding ) => Binding::add_to_linker_async( binding, linker, active_calls, imports ),
			Self::Any( binding ) => Binding::add_to_linker_async( binding, linker, active_calls, imports ),
		}
	}
}

impl<PluginId, Ctx, Instance> From<Binding<PluginId, Ctx, ExactlyOne<PluginId, Instance>, Instance>> for BindingAny<PluginId, Ctx, Instance>
where
	PluginId: std::hash::Hash + Eq + Clone + Send + Sync + 'static,
	Ctx: PluginContext + 'static,
	Instance: Send + 'static,
{
	fn from( binding: Binding<PluginId, Ctx, ExactlyOne<PluginId, Instance>, Instance> ) -> Self {
		Self::ExactlyOne( binding )
	}
}

impl<PluginId, Ctx, Instance> From<Binding<PluginId, Ctx, AtMostOne<PluginId, Instance>, Instance>> for BindingAny<PluginId, Ctx, Instance>
where
	PluginId: std::hash::Hash + Eq + Clone + Send + Sync + 'static,
	Ctx: PluginContext + 'static,
	Instance: Send + 'static,
{
	fn from( binding: Binding<PluginId, Ctx, AtMostOne<PluginId, Instance>, Instance> ) -> Self {
		Self::AtMostOne( binding )
	}
}

impl<PluginId, Ctx, Instance> From<Binding<PluginId, Ctx, AtLeastOne<PluginId, Instance>, Instance>> for BindingAny<PluginId, Ctx, Instance>
where
	PluginId: std::hash::Hash + Eq + Clone + Send + Sync + 'static,
	Ctx: PluginContext + 'static,
	Instance: Send + 'static,
{
	fn from( binding: Binding<PluginId, Ctx, AtLeastOne<PluginId, Instance>, Instance> ) -> Self {
		Self::AtLeastOne( binding )
	}
}

impl<PluginId, Ctx, Instance> From<Binding<PluginId, Ctx, Any<PluginId, Instance>, Instance>> for BindingAny<PluginId, Ctx, Instance>
where
	PluginId: std::hash::Hash + Eq + Clone + Send + Sync + 'static,
	Ctx: PluginContext + 'static,
	Instance: Send + 'static,
{
	fn from( binding: Binding<PluginId, Ctx, Any<PluginId, Instance>, Instance> ) -> Self {
		Self::Any( binding )
	}
}

impl<PluginId, Ctx, Plugins, Instance> Binding<PluginId, Ctx, Plugins, Instance>
where
	PluginId: std::hash::Hash + Eq + Clone + Send + Sync + 'static,
	Ctx: PluginContext + 'static,
	Instance: Send + 'static,
	Plugins: Cardinality<PluginId, Instance>,
	PluginSockets<PluginId, Plugins, Instance>: Send + Sync,
	BindingAny<PluginId, Ctx, Instance>: From<Binding<PluginId, Ctx, Plugins, Instance>>,
{
	/// Converts this binding into a type-erased [`BindingAny`] for heterogeneous socket lists.
	pub fn into_any( self ) -> BindingAny<PluginId, Ctx, Instance> {
		self.into()
	}
}

impl<PluginId, Ctx, Instance> Clone for BindingAny<PluginId, Ctx, Instance>
where
	PluginId: std::hash::Hash + Eq + Clone + Send + Sync + 'static,
	Ctx: PluginContext + 'static,
	Instance: Send + 'static,
{
	/// Creates another handle to the same underlying binding, enabling shared dependencies where
	/// multiple plugins depend on the same binding.
	fn clone( &self ) -> Self {
		match self {
			Self::ExactlyOne( binding ) => Self::ExactlyOne( binding.clone() ),
			Self::AtMostOne( binding ) => Self::AtMostOne( binding.clone() ),
			Self::AtLeastOne( binding ) => Self::AtLeastOne( binding.clone() ),
			Self::Any( binding ) => Self::Any( binding.clone() ),
		}
	}
}
