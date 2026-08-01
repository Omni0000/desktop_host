//! Plugin metadata types.
//!
//! A plugin is a WASM component that implements one [`Binding`]( crate::Binding )
//! (its **plug**) and may depend on zero or more other [`Binding`]( crate::Binding )s
//! (its **sockets**). The plug declares what the plugin exports; sockets declare what
//! the plugin expects to import from other plugins.

use std::collections::HashMap ;
use std::sync::Arc;
use wasmtime::{ Engine, Store };
use wasmtime::component::{ Component, ResourceTable, Linker, Val };

use crate::{ BindingAny, BindingAnyAsync };
use crate::binding::ImportMetadata ;
use crate::plugin_instance::{ AsyncInstanceRuntime, ExportEffects, PluginInstanceAsync, PluginInstanceSync };
use crate::async_scheduler::{ PluginGraph, SchedulerSlot };
use crate::Function ;
use crate::Remap ;

/// Trait for accessing a [`ResourceTable`] from the store's data type.
///
/// Resources that flow between plugins need to be wrapped to track ownership.
/// This trait provides access to the table where those wrapped resources are stored.
/// [`ResourceTable`] is part of the wasmtime component model; see the
/// [wasmtime docs](https://docs.rs/wasmtime/latest/wasmtime/component/) for details.
///
/// # Example
///
/// ```
/// use wasmtime::component::ResourceTable ;
/// use wasm_link::PluginContext ;
///
/// struct MyPluginData {
/// 	resource_table: ResourceTable,
/// 	// ... other fields
/// }
///
/// impl PluginContext for MyPluginData {
/// 	fn resource_table( &mut self ) -> &mut ResourceTable {
/// 		&mut self.resource_table
/// 	}
/// }
/// ```
pub trait PluginContext: Send {
	/// Returns a mutable reference to a resource table.
	fn resource_table( &mut self ) -> &mut ResourceTable ;
}

/// A WASM component bundled with its runtime context, ready for instantiation.
///
/// The component's exports (its **plug**) and imports (its **sockets**) are defined through
/// the [`crate::Binding`], not by this struct.
///
/// The `context` is consumed during linking to become the wasmtime [`Store`]( wasmtime::Store )'s data.
///
/// # Type Parameters
/// - `Ctx`: User context type that will be stored in the wasmtime [`Store`]( wasmtime::Store )
///
/// # Example
///
/// ```
/// # use wasm_link::{ Plugin, PluginContext, ResourceTable, Component, Engine, Linker };
/// # struct Ctx { resource_table: ResourceTable }
/// # impl PluginContext for Ctx {
/// # 	fn resource_table( &mut self ) -> &mut ResourceTable { &mut self.resource_table }
/// # }
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let engine = Engine::default();
/// let linker = Linker::new( &engine );
///
/// let plugin = Plugin::new(
/// 	Component::new( &engine, "(component)" )?,
/// 	Ctx { resource_table: ResourceTable::new() },
/// ).instantiate( &engine, &linker )?;
/// # let _ = plugin;
/// # Ok(())
/// # }
/// ```
#[must_use = "call .instantiate() or .link() to create a PluginInstanceSync"]
pub struct Plugin<Ctx: 'static> {
	/// Compiled WASM component
	component: Component,
	/// User context consumed at load time to become `Store<Ctx>`
	context: Ctx,
	/// Per-interface export name remaps for this plugin
	interface_remaps: HashMap<String, Remap>,
	/// Fuel assigned to the store before component instantiation
	initial_fuel: Option<u64>,
	/// Closure that determines fuel for each function call
	#[allow( clippy::type_complexity )]
	fuel_limiter: Option<Box<dyn FnMut( &mut Store<Ctx>, &str, &str, &Function ) -> u64 + Send>>,
	/// Closure that determines epoch deadline for each function call
	#[allow( clippy::type_complexity )]
	epoch_limiter: Option<Box<dyn FnMut( &mut Store<Ctx>, &str, &str, &Function ) -> u64 + Send>>,
	/// Closure that returns a mutable reference to the `ResourceLimiter` in the context
	#[allow( clippy::type_complexity )]
	memory_limiter: Option<Box<dyn (FnMut( &mut Ctx ) -> &mut dyn wasmtime::ResourceLimiter) + Send + Sync>>,
}

impl<Ctx> Plugin<Ctx>
where
	Ctx: PluginContext + 'static,
{

	/// Creates a new plugin declaration.
	///
	/// Note that the plugin ID is not specified here - it's provided when constructing
	/// the cardinality wrapper that holds this plugin. This is done to prevent duplicate ids.
	pub fn new(
		component: Component,
		context: Ctx,
	) -> Self {
		Self {
			component,
			context,
			interface_remaps: HashMap::new(),
			initial_fuel: None,
			fuel_limiter: None,
			epoch_limiter: None,
			memory_limiter: None,
		}
	}

	/// Sets the fuel available when component instantiation begins.
	///
	/// Instantiation can execute WebAssembly startup code, including complex global,
	/// element, table, and memory initializers and explicit start functions. Any fuel
	/// left after instantiation remains available to subsequent calls. A
	/// [`with_fuel_limiter`](Self::with_fuel_limiter) invocation may inspect or replace
	/// that remainder before a call.
	///
	/// **Warning:** Fuel consumption must be enabled in the [`Engine`]( wasmtime::Engine )
	/// via [`Config::consume_fuel`]( wasmtime::Config::consume_fuel ). If not enabled,
	/// instantiation will fail when the initial fuel is applied.
	///
	/// ```
	/// # use wasm_link::{ Plugin, PluginContext, ResourceTable, Component };
	/// # struct Ctx { resource_table: ResourceTable }
	/// # impl PluginContext for Ctx {
	/// # 	fn resource_table( &mut self ) -> &mut ResourceTable { &mut self.resource_table }
	/// # }
	/// # fn example( component: Component ) {
	/// let plugin = Plugin::new( component, Ctx { resource_table: ResourceTable::new() })
	/// 	.with_initial_fuel( 100_000 );
	/// # let _ = plugin;
	/// # }
	/// ```
	pub fn with_initial_fuel( mut self, fuel: u64 ) -> Self {
		self.initial_fuel = Some( fuel );
		self
	}

	/// Sets a closure that determines the fuel limit for each function call.
	///
	/// The closure receives the store, the interface path (e.g., `"my:package/api"`),
	/// the function name, and the [`Function`] metadata. It returns the fuel to set.
	///
	/// **Warning:** Fuel consumption must be enabled in the [`Engine`]( wasmtime::Engine )
	/// via [`Config::consume_fuel`]( wasmtime::Config::consume_fuel ). If not enabled,
	/// dispatch will fail with a [`RuntimeException`]( crate::DispatchError::RuntimeException )
	/// at call time.
	///
	/// ```
	/// # use wasm_link::{ Plugin, PluginContext, ResourceTable, Component, Engine };
	/// # struct Ctx { resource_table: ResourceTable }
	/// # impl PluginContext for Ctx {
	/// # 	fn resource_table( &mut self ) -> &mut ResourceTable { &mut self.resource_table }
	/// # }
	/// # fn example( component: Component ) {
	/// let plugin = Plugin::new( component, Ctx { resource_table: ResourceTable::new() })
	/// 	.with_fuel_limiter(| _store, _interface, _function, _metadata | 100_000 );
	/// # }
	/// ```
	pub fn with_fuel_limiter( mut self, limiter: impl FnMut( &mut Store<Ctx>, &str, &str, &Function ) -> u64 + Send + 'static ) -> Self {
		self.fuel_limiter = Some( Box::new( limiter ));
		self
	}

	/// Sets a closure that determines the epoch deadline for each function call.
	///
	/// The closure receives the store, the interface path (e.g., `"my:package/api"`),
	/// the function name, and the [`Function`] metadata. It returns the epoch deadline
	/// in ticks.
	///
	/// **Warning:** Epoch interruption must be enabled in the [`Engine`]( wasmtime::Engine )
	/// via [`Config::epoch_interruption`]( wasmtime::Config::epoch_interruption ). If not
	/// enabled, the deadline is silently ignored.
	///
	/// ```
	/// # use wasm_link::{ Plugin, PluginContext, ResourceTable, Component, Engine };
	/// # struct Ctx { resource_table: ResourceTable }
	/// # impl PluginContext for Ctx {
	/// # 	fn resource_table( &mut self ) -> &mut ResourceTable { &mut self.resource_table }
	/// # }
	/// # fn example( component: Component ) {
	/// let plugin = Plugin::new( component, Ctx { resource_table: ResourceTable::new() })
	/// 	.with_epoch_limiter(| _store, _interface, _function, _metadata | 5 );
	/// # }
	/// ```
	pub fn with_epoch_limiter( mut self, limiter: impl FnMut( &mut Store<Ctx>, &str, &str, &Function ) -> u64 + Send + 'static ) -> Self {
		self.epoch_limiter = Some( Box::new( limiter ));
		self
	}

	/// Sets a closure that returns a mutable reference to a [`ResourceLimiter`]( wasmtime::ResourceLimiter )
	/// embedded in the plugin context.
	///
	/// The limiter is installed into the wasmtime [`Store`]( wasmtime::Store ) once at instantiation
	/// and controls memory and table growth for the lifetime of the plugin.
	///
	/// The [`ResourceLimiter`]( wasmtime::ResourceLimiter ) must be stored inside the context type `Ctx`
	/// so that wasmtime can access it through a `&mut Ctx` reference.
	///
	/// ```
	/// # use wasm_link::{ Plugin, PluginContext, ResourceTable, Component, Engine };
	/// # struct Ctx { resource_table: ResourceTable, limiter: MyLimiter }
	/// # impl PluginContext for Ctx {
	/// # 	fn resource_table( &mut self ) -> &mut ResourceTable { &mut self.resource_table }
	/// # }
	/// # struct MyLimiter;
	/// # impl wasmtime::ResourceLimiter for MyLimiter {
	/// # 	fn memory_growing( &mut self, _: usize, _: usize, _: Option<usize> ) -> wasmtime::Result<bool> { Ok( true ) }
	/// # 	fn table_growing( &mut self, _: usize, _: usize, _: Option<usize> ) -> wasmtime::Result<bool> { Ok( true ) }
	/// # }
	/// # fn example( component: Component ) {
	/// let plugin = Plugin::new( component, Ctx { resource_table: ResourceTable::new(), limiter: MyLimiter })
	/// 	.with_memory_limiter(| ctx | &mut ctx.limiter );
	/// # }
	/// ```
	pub fn with_memory_limiter(
		mut self,
		limiter: impl (FnMut( &mut Ctx ) -> &mut dyn wasmtime::ResourceLimiter) + Send + Sync + 'static,
	) -> Self {
		self.memory_limiter = Some( Box::new( limiter ));
		self
	}

	/// Sets interface export remaps for this plugin.
	///
	/// Use this when a plugin implements the same interface types as its binding
	/// but exports one or more interfaces or functions under different names.
	///
	/// The outer map is a lookup table from requested interface name to [`Remap`].
	/// Each [`Remap`] describes where that requested interface, and optionally
	/// requested items inside it, are found in this plugin's exports.
	///
	/// All remap tables use the same direction:
	///
	/// ```text
	/// requested name -> exported name
	/// ```
	///
	/// ```
	/// # use std::collections::HashMap ;
	/// # use wasm_link::{ Plugin, PluginContext, ResourceTable, Component, Engine, Remap };
	/// # struct Ctx { resource_table: ResourceTable }
	/// # impl PluginContext for Ctx {
	/// # 	fn resource_table( &mut self ) -> &mut ResourceTable { &mut self.resource_table }
	/// # }
	/// # fn example( engine: &Engine ) -> Result<(), Box<dyn std::error::Error>> {
	/// let plugin = Plugin::new(
	/// 	Component::new( engine, "(component)" )?,
	/// 	Ctx { resource_table: ResourceTable::new() },
	/// ).remap_interfaces( HashMap::from([
	/// 	( "root".to_string(), Remap::found_as( "legacy-root" )),
	/// ]));
	/// # let _ = plugin ;
	/// # Ok(())
	/// # }
	/// ```
	pub fn remap_interfaces( mut self, interface_remaps: HashMap<String, Remap> ) -> Self {
		self.interface_remaps = interface_remaps ;
		self
	}

	/// Links this plugin with its socket bindings and instantiates it.
	///
	/// Takes ownership of the `linker` because socket bindings are added to it. If you need
	/// to reuse the same linker for multiple plugins, clone it before passing it in.
	///
	/// # Type Parameters
	/// - `PluginId`: Must implement `Into<Val>` so plugin IDs can be passed to WASM when
	/// 	dispatching to multi-plugin sockets (the ID identifies which plugin produced each result).
	///
	/// # Errors
	/// Returns an error if linking or instantiation fails.
	pub fn link<PluginId, Sockets>(
		self,
		engine: &Engine,
		mut linker: Linker<Ctx>,
		sockets: Sockets,
	) -> Result<PluginInstanceSync<Ctx>, wasmtime::Error>
	where
		PluginId: Eq + std::hash::Hash + Clone + std::fmt::Debug + Send + Sync + Into<Val> + 'static,
		Sockets: IntoIterator,
		Sockets::Item: Into<BindingAny<PluginId, Ctx>>,
	{
		let sockets = sockets.into_iter().map( Into::into ).collect::<Vec<_>>();
		let graph = PluginGraph::new( sockets.iter().flat_map( BindingAny::graphs ).collect() );
		sockets.iter().try_for_each(| binding | binding.add_to_linker( &mut linker ))?;
		self.instantiate_with_graph( engine, &linker, graph )
	}

	/// Asynchronously links this plugin with its socket bindings and instantiates it.
	///
	/// Use this variant when this plugin imports functions asynchronously or any
	/// destination socket may suspend. Socket bindings may contain either
	/// [`PluginInstanceSync`] or [`PluginInstanceAsync`].
	/// The future returned by dispatch drives every involved plugin store
	/// cooperatively on its current async task.
	///
	/// # Example
	///
	/// ```
	/// # use wasm_link::{ BindingAnyAsync, Component, Engine, Linker, Plugin, PluginContext, ResourceTable };
	/// # struct Context { table: ResourceTable }
	/// # impl PluginContext for Context { fn resource_table( &mut self ) -> &mut ResourceTable { &mut self.table } }
	/// # fn main() -> Result<(), Box<dyn std::error::Error>> { futures::executor::block_on( async {
	/// let engine = Engine::default();
	/// let linker = Linker::new( &engine );
	/// let instance = Plugin::new(
	/// 	Component::new( &engine, "(component)" )?,
	/// 	Context { table: ResourceTable::new() },
	/// ).link_async(
	/// 	&engine,
	/// 	linker,
	/// 	Vec::<BindingAnyAsync<String, Context>>::new(),
	/// ).await?;
	/// # let _ = instance;
	/// # Ok(()) }) }
	/// ```
	///
	/// # Errors
	/// Returns an error if linking or instantiation fails.
	pub async fn link_async<PluginId, Sockets>(
		self,
		engine: &Engine,
		mut linker: Linker<Ctx>,
		sockets: Sockets,
	) -> Result<PluginInstanceAsync<Ctx>, wasmtime::Error>
	where
		PluginId: Eq + std::hash::Hash + Clone + std::fmt::Debug + Send + Sync + Into<Val> + 'static,
		Sockets: IntoIterator,
		Sockets::Item: Into<BindingAnyAsync<PluginId, Ctx>>,
	{
		let mut import_asyncness = HashMap::new();
		for ( import_name, import ) in self.component.component_type().imports( engine ) {
			let wasmtime::component::types::ComponentItem::ComponentInstance( instance ) = import.ty else { continue; };
			let functions = instance.exports( engine ).filter_map(|( name, export )| match export.ty {
				wasmtime::component::types::ComponentItem::ComponentFunc( function ) =>
					Some(( name.to_string(), function.async_() )),
				_ => None,
			}).collect::<HashMap<_, _>>();
			let interface = import.implements.unwrap_or( import_name );
			import_asyncness.insert( unversioned( interface ).to_string(), ImportMetadata {
				linker_name: import_name.to_string(),
				functions,
			});
		}
		let sockets = sockets.into_iter().map( Into::into ).collect::<Vec<_>>();
		let graph = PluginGraph::new( sockets.iter().flat_map( BindingAnyAsync::graphs ).collect() );
		let scheduler_slot = SchedulerSlot::new();
		sockets.iter().try_for_each(| binding |
			binding.add_to_linker( &mut linker, graph.key(), &scheduler_slot, &import_asyncness )
		)?;
		self.instantiate_async_with_graph( engine, &linker, graph, scheduler_slot ).await
	}

	/// A convenience alias for [`Plugin::link`] with 0 sockets
	///
	/// # Errors
	/// Returns an error if instantiation fails.
	pub fn instantiate(
		self,
		engine: &Engine,
		linker: &Linker<Ctx>
	) -> Result<PluginInstanceSync<Ctx>, wasmtime::Error> {
		self.instantiate_with_graph( engine, linker, PluginGraph::new( Vec::new() ))
	}

	fn instantiate_with_graph(
		self,
		engine: &Engine,
		linker: &Linker<Ctx>,
		graph: PluginGraph,
	) -> Result<PluginInstanceSync<Ctx>, wasmtime::Error> {
		let export_effects = component_export_effects( &self.component, engine );
		let mut store = Store::new( engine, self.context );
		if let Some( fuel ) = self.initial_fuel { store.set_fuel( fuel )?; }
		if let Some( limiter ) = self.memory_limiter { store.limiter( limiter ); }
		let instance = linker.instantiate( &mut store, &self.component )?;
		Ok( PluginInstanceSync::new_sync(
			store,
			instance,
			self.interface_remaps,
			export_effects,
			graph,
			self.fuel_limiter,
			self.epoch_limiter,
		))
	}

	/// Asynchronously instantiates this plugin.
	///
	/// This variant is required for WIT async functions, asynchronous host functions,
	/// and plugins that will be used in a graph created with [`link_async`](Self::link_async).
	/// The plugin's [`Store`](wasmtime::Store) is driven by the future returned from
	/// asynchronous dispatch; no executor or worker thread is required.
	/// Wasmtime concurrency support, which is enabled by default, must not be disabled
	/// on the `engine` used for asynchronous instances.
	///
	/// # Example
	///
	/// ```
	/// # use wasm_link::{ Component, Engine, Linker, Plugin, PluginContext, ResourceTable };
	/// # struct Context { table: ResourceTable }
	/// # impl PluginContext for Context { fn resource_table( &mut self ) -> &mut ResourceTable { &mut self.table } }
	/// # fn main() -> Result<(), Box<dyn std::error::Error>> { futures::executor::block_on( async {
	/// let engine = Engine::default();
	/// let linker = Linker::new( &engine );
	/// let instance = Plugin::new(
	/// 	Component::new( &engine, "(component)" )?,
	/// 	Context { table: ResourceTable::new() },
	/// ).instantiate_async( &engine, &linker ).await?;
	/// # let _ = instance;
	/// # Ok(()) }) }
	/// ```
	///
	/// # Errors
	/// Returns an error if instantiation fails.
	pub async fn instantiate_async(
		self,
		engine: &Engine,
		linker: &Linker<Ctx>,
	) -> Result<PluginInstanceAsync<Ctx>, wasmtime::Error> {
		self.instantiate_async_with_graph(
			engine,
			linker,
			PluginGraph::new( Vec::new() ),
			SchedulerSlot::new(),
		).await
	}

	async fn instantiate_async_with_graph(
		self,
		engine: &Engine,
		linker: &Linker<Ctx>,
		graph: PluginGraph,
		scheduler_slot: Arc<SchedulerSlot<Ctx>>,
	) -> Result<PluginInstanceAsync<Ctx>, wasmtime::Error> {
		let export_effects = component_export_effects( &self.component, engine );
		let mut store = Store::new( engine, self.context );
		if let Some( fuel ) = self.initial_fuel { store.set_fuel( fuel )?; }
		if let Some( limiter ) = self.memory_limiter { store.limiter( limiter ); }
		let instance = linker.instantiate_async( &mut store, &self.component ).await?;
		Ok( PluginInstanceAsync::new(
			store,
			instance,
			self.interface_remaps,
			export_effects,
			AsyncInstanceRuntime::new( graph, scheduler_slot ),
			self.fuel_limiter,
			self.epoch_limiter,
		))
	}

}

fn component_export_effects( component: &Component, engine: &Engine ) -> ExportEffects {
	component.component_type().exports( engine ).filter_map(|( interface, export )| {
		let wasmtime::component::types::ComponentItem::ComponentInstance( instance ) = export.ty else {
			return None;
		};
		let functions = instance.exports( engine ).filter_map(|( name, export )| match export.ty {
			wasmtime::component::types::ComponentItem::ComponentFunc( function ) =>
				Some(( name.to_string(), function.async_() )),
			_ => None,
		}).collect();
		let interface = export.implements.unwrap_or( interface );
		Some(( unversioned( interface ).to_string(), functions ))
	}).collect()
}

fn unversioned( interface: &str ) -> &str {
	match interface.split_once( '@' ) {
		Some(( interface, _ )) => interface,
		None => interface,
	}
}

impl<Ctx: std::fmt::Debug + 'static> std::fmt::Debug for Plugin<Ctx> {
	fn fmt( &self, f: &mut std::fmt::Formatter<'_> ) -> std::fmt::Result {
		f.debug_struct( "Plugin" )
			.field( "component", &"<Component>" )
			.field( "context", &self.context )
			.field( "interface_remaps", &self.interface_remaps )
			.field( "initial_fuel", &self.initial_fuel )
			.field( "fuel_limiter", &self.fuel_limiter.as_ref().map(| _ | "<closure>" ))
			.field( "epoch_limiter", &self.epoch_limiter.as_ref().map(| _ | "<closure>" ))
			.field( "memory_limiter", &self.memory_limiter.as_ref().map(| _ | "<closure>" ))
			.finish_non_exhaustive()
	}
}
