use std::sync::Arc ;
use std::collections::{ HashMap, HashSet };
use futures::lock::Mutex ;
use thiserror::Error ;
use wasmtime::{ AsContextMut, component::{ Linker, ResourceType, Val }};

use crate::{ Binding, PluginContext, PluginInstanceAsync, PluginInstanceSync };
use crate::cardinality::Cardinality ;
use crate::linker::{
	DispatchTarget,
	dispatch_all,
	dispatch_all_async,
	dispatch_all_async_sync,
	dispatch_method,
	dispatch_method_async,
	dispatch_method_async_sync,
};
use crate::resource_wrapper::ResourceWrapper ;

#[derive( Debug, Error )]
enum LinkError {
	#[error( "synchronously instantiated plugin exposes async export `{interface}.{function}`" )]
	SyncInstanceAsyncExport { interface: String, function: String },
}

/// A single WIT interface within a [`Binding`].
///
/// Each interface declares functions and resources that implementers must export.
/// Note that the interface name is not a part of the struct but rather a key in
/// a hash map provided to the Binding constructor. This is to prevent duplicate
/// interface names.
///
/// ```
/// # use std::collections::{ HashMap, HashSet };
/// # use wasm_link::{ Binding, Interface, PluginContext, PluginInstanceSync, ResourceTable };
/// # use wasm_link::cardinality::AtMostOne ;
/// # struct Ctx { resource_table: ResourceTable }
/// # impl PluginContext for Ctx {
/// # 	fn resource_table( &mut self ) -> &mut ResourceTable { &mut self.resource_table }
/// # }
/// let binding: Binding<String, Ctx, AtMostOne<String, PluginInstanceSync<Ctx>>> = Binding::new(
/// 	"my:package",
/// 	HashMap::from([
/// 		( "interface-a".to_string(), Interface::new( HashMap::new(), HashSet::new() )),
/// 		( "interface-b".to_string(), Interface::new( HashMap::new(), HashSet::new() )),
/// 	]),
/// 	AtMostOne( None ),
/// );
/// # let _ = binding;
/// ```
#[derive( Debug, Clone, Default )]
pub struct Interface {
	/// Functions exported by this interface
	functions: HashMap<String, Function>,
	/// Resource types defined by this interface
	resources: HashSet<String>,
}

impl Interface {
	/// Creates a new interface declaration.
	pub fn new(
		functions: HashMap<String, Function>,
		resources: HashSet<String>,
	) -> Self {
		Self { functions, resources }
	}

	#[inline]
	pub(crate) fn function( &self, name: &str ) -> Option<&Function> {
		self.functions.get( name )
	}

	pub(crate) fn function_names( &self ) -> impl Iterator<Item = &str> {
		self.functions.keys().map( String::as_str )
	}

	#[inline]
	pub(crate) fn add_to_linker<PluginId, Ctx, Plugins>(
		&self,
		linker: &mut Linker<Ctx>,
		interface_ident: &str,
		interface_name: &str,
		binding: &Binding<PluginId, Ctx, Plugins, PluginInstanceSync<Ctx>>,
	) -> Result<(), wasmtime::Error>
	where
		PluginId: std::hash::Hash + Eq + Clone + Send + Sync + Into<Val> + 'static,
		Ctx: PluginContext,
		Plugins: Cardinality<PluginId, PluginInstanceSync<Ctx>> + 'static,
		<Plugins as Cardinality<PluginId, PluginInstanceSync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceSync<Ctx>>>>: Send + Sync,
		<Plugins as Cardinality<PluginId, PluginInstanceSync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceSync<Ctx>>>>: Cardinality<PluginId, Arc<Mutex<PluginInstanceSync<Ctx>>>>,
		<<Plugins as Cardinality<PluginId, PluginInstanceSync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceSync<Ctx>>>> as Cardinality<PluginId, Arc<Mutex<PluginInstanceSync<Ctx>>>>>::Rebind<Val>: Into<Val>,
	{
		let mut linker_root = linker.root();
		let mut linker_instance = linker_root.instance( interface_ident )?;
		let package_name = binding.package_name();

		self.functions.iter().try_for_each(|( name, metadata )| {

			let package_name_clone = package_name.to_string();
			let interface_name_clone = interface_name.to_string();
			let binding_clone = binding.clone();
			let name_clone = name.clone();
			let metadata_clone = metadata.clone();

			macro_rules! link {( $dispatch: expr ) => {
				linker_instance.func_new( name, move | ctx, _ty, args, results | Ok(
					results[0] = $dispatch( &binding_clone, ctx, &package_name_clone, &interface_name_clone, &name_clone, &metadata_clone, args )
				))
			}}

			match metadata.kind() {
				FunctionKind::Freestanding => link!( dispatch_all ),
				FunctionKind::Method => link!( dispatch_method ),
			}

		})?;

		for resource in &self.resources {
			linker_instance.resource( resource, ResourceType::host::<Arc<ResourceWrapper<PluginId>>>(), ResourceWrapper::<PluginId>::drop )?;
		}

		Ok(())

	}

	#[inline]
	pub(crate) fn add_to_linker_async_sync<PluginId, Ctx, Plugins>(
		&self,
		linker: &mut Linker<Ctx>,
		interface_ident: &str,
		interface_name: &str,
		binding: &Binding<PluginId, Ctx, Plugins, PluginInstanceSync<Ctx>>,
		imports: Option<&HashMap<String, bool>>,
	) -> Result<(), wasmtime::Error>
	where
		PluginId: std::hash::Hash + Eq + Clone + Send + Sync + Into<Val> + 'static,
		Ctx: PluginContext,
		Plugins: Cardinality<PluginId, PluginInstanceSync<Ctx>> + 'static,
		<Plugins as Cardinality<PluginId, PluginInstanceSync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceSync<Ctx>>>>: Send + Sync,
		<Plugins as Cardinality<PluginId, PluginInstanceSync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceSync<Ctx>>>>: Cardinality<PluginId, Arc<Mutex<PluginInstanceSync<Ctx>>>>,
		<<Plugins as Cardinality<PluginId, PluginInstanceSync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceSync<Ctx>>>> as Cardinality<PluginId, Arc<Mutex<PluginInstanceSync<Ctx>>>>>::Rebind<Val>: Into<Val> + Send,
	{
		let mut linker_root = linker.root();
		let mut linker_instance = linker_root.instance( interface_ident )?;
		let package_name = binding.package_name();

		self.functions.iter().try_for_each(|( name, metadata )| {
			let destination_is_async = binding.export_is_async( interface_name, name );
			if destination_is_async {
				return Err( LinkError::SyncInstanceAsyncExport {
					interface: interface_ident.to_string(),
					function: name.clone(),
				}.into() );
			}
			let import_is_async = imports.and_then(| functions | functions.get( name )).copied().unwrap_or( false );
			let package_name = package_name.to_string();
			let interface_name = interface_name.to_string();
			let binding = binding.clone();
			let function_name = name.clone();
			let function = metadata.clone();

			macro_rules! link_sync {( $dispatch: expr ) => {
				linker_instance.func_new_async( name, move | ctx, _ty, args, results | {
					let value = $dispatch(
						&binding,
						ctx,
						&package_name,
						&interface_name,
						&function_name,
						&function,
						args,
					);
					Box::new( async move {
						results[0] = value;
						Ok(())
					})
				})
			}}
			macro_rules! link_concurrent {( $dispatch: expr ) => {{
				linker_instance.func_new_concurrent( name, move | ctx, _ty, args, results | {
					let package_name = package_name.clone();
					let interface_name = interface_name.clone();
					let binding = binding.clone();
					let function_name = function_name.clone();
					let function = function.clone();
					Box::pin( async move {
						results[0] = ctx.with(| mut access | $dispatch(
							&binding,
							access.as_context_mut(),
							&package_name,
							&interface_name,
							&function_name,
							&function,
							args,
						));
						Ok(())
					})
				})
			}}}

			match ( import_is_async, metadata.kind() ) {
				( true, FunctionKind::Freestanding ) => link_concurrent!( dispatch_all ),
				( true, FunctionKind::Method ) => link_concurrent!( dispatch_method ),
				( false, FunctionKind::Freestanding ) => link_sync!( dispatch_all ),
				( false, FunctionKind::Method ) => link_sync!( dispatch_method ),
			}
		})?;

		for resource in &self.resources {
			linker_instance.resource( resource, ResourceType::host::<Arc<ResourceWrapper<PluginId>>>(), ResourceWrapper::<PluginId>::drop )?;
		}

		Ok(())
	}

	#[inline]
	pub(crate) fn add_to_linker_async<PluginId, Ctx, Plugins>(
		&self,
		linker: &mut Linker<Ctx>,
		interface_ident: &str,
		interface_name: &str,
		binding: &Binding<PluginId, Ctx, Plugins, PluginInstanceAsync<Ctx>>,
		dispatch: &crate::async_scheduler::LinkDispatchContext<'_, Ctx>,
		imports: Option<&HashMap<String, bool>>,
	) -> Result<(), wasmtime::Error>
	where
		PluginId: std::hash::Hash + Eq + Clone + Send + Sync + Into<Val> + 'static,
		Ctx: PluginContext,
		Plugins: Cardinality<PluginId, PluginInstanceAsync<Ctx>> + 'static,
		<Plugins as Cardinality<PluginId, PluginInstanceAsync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceAsync<Ctx>>>>: Send + Sync,
		<Plugins as Cardinality<PluginId, PluginInstanceAsync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceAsync<Ctx>>>>: Cardinality<PluginId, Arc<Mutex<PluginInstanceAsync<Ctx>>>>,
		<<Plugins as Cardinality<PluginId, PluginInstanceAsync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceAsync<Ctx>>>> as Cardinality<PluginId, Arc<Mutex<PluginInstanceAsync<Ctx>>>>>::Rebind<Val>: Into<Val> + Send,
	{
		let mut linker_root = linker.root();
		let mut linker_instance = linker_root.instance( interface_ident )?;
		let package_name = binding.package_name();
		let caller = dispatch.caller;
		let scheduler_slot = dispatch.scheduler_slot;

		self.functions.iter().try_for_each(|( name, metadata )| {
			let import_is_async = imports.and_then(| functions | functions.get( name )).copied().unwrap_or( false );
			let package_name = package_name.to_string();
			let interface_name = interface_name.to_string();
			let binding = binding.clone();
			let function_name = name.clone();
			let function = metadata.clone();

			macro_rules! link_concurrent {( $dispatch: expr ) => {{
				let caller = caller;
				let scheduler_slot = Arc::clone( scheduler_slot );
				linker_instance.func_new_concurrent( name, move | ctx, _ty, args, results | {
					let package_name = package_name.clone();
					let interface_name = interface_name.clone();
					let binding = binding.clone();
					let function_name = function_name.clone();
					let function = function.clone();
					let caller = caller;
					let scheduler_slot = Arc::clone( &scheduler_slot );
					Box::pin( async move {
						let scheduler = scheduler_slot.require()?;
						let path = scheduler.execution_path( caller );
						let target = DispatchTarget::new( &package_name, &interface_name, &function_name, &function );
						results[0] = $dispatch(
							&binding, &scheduler, caller, path, ctx, &target, args,
						).await;
						Ok(())
					})
				})
			}}}

			macro_rules! link_sync {( $dispatch: expr ) => {{
				let caller = caller;
				let scheduler_slot = Arc::clone( scheduler_slot );
				linker_instance.func_new_async( name, move | ctx, _ty, args, results | {
					let package_name = package_name.clone();
					let interface_name = interface_name.clone();
					let binding = binding.clone();
					let function_name = function_name.clone();
					let function = function.clone();
					let caller = caller;
					let scheduler_slot = Arc::clone( &scheduler_slot );
					Box::new( async move {
						let scheduler = scheduler_slot.require()?;
						let path = scheduler.execution_path( caller );
						let target = DispatchTarget::new( &package_name, &interface_name, &function_name, &function );
						results[0] = $dispatch(
							&binding, &scheduler, caller, path, ctx, &target, args,
						).await;
						Ok(())
					})
				})
			}}}

			match ( import_is_async, metadata.kind() ) {
				( true, FunctionKind::Freestanding ) => link_concurrent!( dispatch_all_async ),
				( true, FunctionKind::Method ) => link_concurrent!( dispatch_method_async ),
				( false, FunctionKind::Freestanding ) => link_sync!( dispatch_all_async_sync ),
				( false, FunctionKind::Method ) => link_sync!( dispatch_method_async_sync ),
			}
		})?;

		for resource in &self.resources {
			linker_instance.resource( resource, ResourceType::host::<Arc<ResourceWrapper<PluginId>>>(), ResourceWrapper::<PluginId>::drop )?;
		}

		Ok(())
	}

}

/// Denotes whether a function is freestanding or a resource method.
/// Constructors are treated as freestanding functions.
///
/// Determines how dispatch is routed during cross-plugin calls:
/// freestanding functions broadcast to all plugins, while methods
/// route to the specific plugin that owns the resource.
#[derive( Debug, Clone, Copy, Eq, PartialEq )]
pub enum FunctionKind {
	/// A freestanding function — dispatched to all plugins.
	Freestanding,
	/// A resource method (has a `self` parameter) — routed to the plugin that owns the resource.
	Method,
}

/// Metadata about a function declared by an interface.
///
/// Provides routing and return-value information for cross-plugin dispatch.
/// Asyncness is read from the source import and destination exports at link time;
/// it is not part of binding metadata.
#[derive( Debug, Clone )]
pub struct Function {
	/// Whether this function is freestanding or a resource method.
	kind: FunctionKind,
	/// The function's return kind for dispatch handling
	return_kind: ReturnKind,
}

impl Function {
	/// Creates a new function metadata entry.
	pub fn new(
		kind: FunctionKind,
		return_kind: ReturnKind,
	) -> Self {
		Self { kind, return_kind }
	}

	/// The function's return kind for dispatch handling.
	pub fn return_kind( &self ) -> ReturnKind { self.return_kind }

	/// Whether this function is freestanding or a resource method.
	pub fn kind( &self ) -> FunctionKind { self.kind }

}

/// Categorizes a function's return for dispatch handling.
///
/// Determines how return values are processed during cross-plugin dispatch.
/// Resources require special wrapping to track ownership across plugin
/// boundaries, while plain data can be passed through directly.
///
/// # Choosing the Right Variant
///
/// **When uncertain, use [`MayContainResources`]( Self::MayContainResources ).**
/// Using [`AssumeNoResources`]( Self::AssumeNoResources ) when resources are
/// actually present will cause resource handles to be passed through unwrapped
/// causing runtime exceptions.
///
/// [`AssumeNoResources`]( Self::AssumeNoResources ) is a performance optimization
/// that skips the wrapping step. Only use it when you are certain the return type
/// contains no resource handles anywhere in its structure (including nested within
/// records, variants, lists, etc.).
#[derive( Copy, Clone, Eq, PartialEq, Hash, Debug, Default )]
pub enum ReturnKind {
	/// Function returns nothing (void).
	#[default] Void,
	/// Function may return resource handles - always wraps safely.
	///
	/// Use this variant whenever resources might be present in the return value,
	/// or when you're unsure. The performance overhead of wrapping is preferable
	/// to the undefined behavior caused by unwrapped resource handles.
	MayContainResources,
	/// Assumes no resource handles are present - skips wrapping for performance.
	///
	/// **Warning:** Only use this if you are certain no resources are present.
	/// If resources are returned but this variant is used, resource handles will
	/// not be wrapped correctly, potentially causing undefined behavior in plugins.
	/// When in doubt, use [`MayContainResources`](Self::MayContainResources) instead.
	AssumeNoResources,
}

impl std::fmt::Display for ReturnKind {
	fn fmt( &self, f: &mut std::fmt::Formatter ) -> Result<(), std::fmt::Error> {
		match self {
			Self::Void => write!( f, "Function returns no data" ),
			Self::MayContainResources => write!( f, "Return type may contain resources" ),
			Self::AssumeNoResources => write!( f, "Function is assumed to not return any resources" ),
		}
	}
}
