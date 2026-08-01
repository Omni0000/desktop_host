use std::sync::Arc ;
use futures::lock::Mutex ;
use wasmtime::{ AsContextMut, StoreContextMut };
use wasmtime::component::{ Accessor, Val };

use crate::{ Binding, Function, FunctionKind, ReturnKind, PluginContext, DispatchError };
use crate::cardinality::Cardinality ;
use crate::plugin_instance::{ PluginInstanceAsync, PluginInstanceSync };
use crate::async_scheduler::{ DispatchContext, AsyncScheduler, ExecutionPathId, PluginKey };
use super::resource_wrapper::ResourceWrapper ;



pub(crate) struct DispatchTarget<'a> {
	package_name: &'a str,
	interface_name: &'a str,
	function_name: &'a str,
	function: &'a Function,
}

impl<'a> DispatchTarget<'a> {
	pub(crate) fn new( package_name: &'a str, interface_name: &'a str, function_name: &'a str, function: &'a Function ) -> Self {
		Self { package_name, interface_name, function_name, function }
	}
}

/// Dispatches a non-method function call to all plugins
pub(crate) fn dispatch_all<PluginId, Ctx, Plugins>(
	binding: &Binding<PluginId, Ctx, Plugins, PluginInstanceSync<Ctx>>,
	mut ctx: StoreContextMut<Ctx>,
	package_name: &str,
	interface_name: &str,
	function_name: &str,
	function: &Function,
	data: &[Val],
) -> Val
where
	PluginId: Clone + std::hash::Hash + Eq + Send + Sync + Into<Val> + 'static,
	Ctx: PluginContext,
	Plugins: Cardinality<PluginId, PluginInstanceSync<Ctx>>,
	<Plugins as Cardinality<PluginId, PluginInstanceSync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceSync<Ctx>>>>: Send + Sync,
	<Plugins as Cardinality<PluginId, PluginInstanceSync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceSync<Ctx>>>>: Cardinality<PluginId, Arc<Mutex<PluginInstanceSync<Ctx>>>>,
	<<Plugins as Cardinality<PluginId, PluginInstanceSync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceSync<Ctx>>>> as Cardinality<PluginId, Arc<Mutex<PluginInstanceSync<Ctx>>>>>::Rebind<Val>: Into<Val>,
{
	debug_assert_eq!( function.kind(), FunctionKind::Freestanding );
	let target = DispatchTarget {
		package_name,
		interface_name,
		function_name,
		function,
	};
	binding.plugins().map(| plugin_id, plugin | Val::Result(
		match dispatch_of(
			&mut ctx,
			plugin_id.clone(),
			plugin,
			&target,
			data,
		) {
			Ok( val ) => Ok( Some( Box::new( val ))),
			Err( err ) => Err( Some( Box::new( err.into() ))),
		}
	)).into()
}

/// Dispatches a method function call, routing to the correct plugin.
pub(crate) fn dispatch_method<PluginId, Ctx, Plugins>(
	binding: &Binding<PluginId, Ctx, Plugins, PluginInstanceSync<Ctx>>,
	ctx: StoreContextMut<Ctx>,
	package_name: &str,
	interface_name: &str,
	function_name: &str,
	function: &Function,
	data: &[Val],
) -> Val
where
	PluginId: Clone + std::hash::Hash + Eq + Send + Sync + 'static,
	Ctx: PluginContext,
	Plugins: Cardinality<PluginId, PluginInstanceSync<Ctx>>,
	<Plugins as Cardinality<PluginId, PluginInstanceSync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceSync<Ctx>>>>: Send + Sync,
	<Plugins as Cardinality<PluginId, PluginInstanceSync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceSync<Ctx>>>>: Cardinality<PluginId, Arc<Mutex<PluginInstanceSync<Ctx>>>>,
{
	debug_assert_eq!( function.kind(), FunctionKind::Method );
	Val::Result( match route_method(
		binding,
		ctx,
		package_name,
		interface_name,
		function_name,
		function,
		data,
	) {
		Ok( val ) => Ok( Some( Box::new( val ))),
		Err( err ) => Err( Some( Box::new( err.into() ))),
	})
}

#[inline]
fn dispatch_of<PluginId, Ctx>(
	ctx: &mut StoreContextMut<Ctx>,
	plugin_id: PluginId,
	plugin: &Arc<Mutex<PluginInstanceSync<Ctx>>>,
	target: &DispatchTarget<'_>,
	data: &[Val],
) -> Result<Val, DispatchError>
where
	PluginId: Clone + std::hash::Hash + Eq + Send + Sync + 'static,
	Ctx: PluginContext,
{

	let mut lock = plugin.try_lock().ok_or( DispatchError::LockRejected )?;
	let result = lock.dispatch( target.package_name, target.interface_name, target.function_name, target.function, data )?;

	Ok( match target.function.return_kind() {
		ReturnKind::Void | ReturnKind::AssumeNoResources => result,
		ReturnKind::MayContainResources => wrap_resources( result, plugin_id, ctx )?,
	})
}

#[inline]
fn route_method<PluginId, Ctx, Plugins>(
	binding: &Binding<PluginId, Ctx, Plugins, PluginInstanceSync<Ctx>>,
	mut ctx: StoreContextMut<Ctx>,
	package_name: &str,
	interface_name: &str,
	function_name: &str,
	function: &Function,
	data: &[Val],
) -> Result<Val, DispatchError>
where
	PluginId: Clone + std::hash::Hash + Eq + Send + Sync + 'static,
	Ctx: PluginContext,
	Plugins: Cardinality<PluginId, PluginInstanceSync<Ctx>>,
	<Plugins as Cardinality<PluginId, PluginInstanceSync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceSync<Ctx>>>>: Send + Sync,
	<Plugins as Cardinality<PluginId, PluginInstanceSync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceSync<Ctx>>>>: Cardinality<PluginId, Arc<Mutex<PluginInstanceSync<Ctx>>>>,
{

	let handle = match data.first() {
		Some( Val::Resource( handle )) => Ok( handle ),
		_ => Err( DispatchError::InvalidArgumentList ),
	}?;

	let resource = ResourceWrapper::<PluginId>::from_handle( *handle, &mut ctx )?;
	let plugin = binding.plugins().get( &resource.plugin_id ).ok_or( DispatchError::InvalidArgumentList )?;
	let plugin_id = resource.plugin_id.clone();

	let mut data = Vec::from( data );
	data[0] = Val::Resource( resource.resource_handle );
	let target = DispatchTarget {
		package_name,
		interface_name,
		function_name,
		function,
	};

	dispatch_of(
		&mut ctx,
		plugin_id,
		plugin,
		&target,
		&data,
	)

}

/// Asynchronously dispatches a non-method function call to all plugins.
pub(crate) async fn dispatch_all_async<PluginId, Ctx, Plugins>(
	binding: &Binding<PluginId, Ctx, Plugins, PluginInstanceAsync<Ctx>>,
	scheduler: &AsyncScheduler<Ctx>,
	caller: PluginKey,
	path: ExecutionPathId,
	ctx: &Accessor<Ctx>,
	target: &DispatchTarget<'_>,
	data: &[Val],
) -> Val
where
	PluginId: Clone + std::hash::Hash + Eq + Send + Sync + Into<Val> + 'static,
	Ctx: PluginContext,
	Plugins: Cardinality<PluginId, PluginInstanceAsync<Ctx>>,
	<Plugins as Cardinality<PluginId, PluginInstanceAsync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceAsync<Ctx>>>>: Send + Sync,
	<Plugins as Cardinality<PluginId, PluginInstanceAsync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceAsync<Ctx>>>>: Cardinality<PluginId, Arc<Mutex<PluginInstanceAsync<Ctx>>>>,
	<<Plugins as Cardinality<PluginId, PluginInstanceAsync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceAsync<Ctx>>>> as Cardinality<PluginId, Arc<Mutex<PluginInstanceAsync<Ctx>>>>>::Rebind<Val>: Into<Val> + Send,
{
	debug_assert_eq!( target.function.kind(), FunctionKind::Freestanding );
	binding.plugins().map_async(| plugin_id, plugin | async {
		Val::Result( match dispatch_of_async( DispatchContext::new( scheduler, caller, path ), ctx, plugin_id, plugin, target, data ).await {
			Ok( val ) => Ok( Some( Box::new( val ))),
			Err( err ) => Err( Some( Box::new( err.into() ))),
		})
	}).await.into()
}

/// Asynchronously dispatches a method call to the plugin owning its resource.
pub(crate) async fn dispatch_method_async<PluginId, Ctx, Plugins>(
	binding: &Binding<PluginId, Ctx, Plugins, PluginInstanceAsync<Ctx>>,
	scheduler: &AsyncScheduler<Ctx>,
	caller: PluginKey,
	path: ExecutionPathId,
	ctx: &Accessor<Ctx>,
	target: &DispatchTarget<'_>,
	data: &[Val],
) -> Val
where
	PluginId: Clone + std::hash::Hash + Eq + Send + Sync + 'static,
	Ctx: PluginContext,
	Plugins: Cardinality<PluginId, PluginInstanceAsync<Ctx>>,
	<Plugins as Cardinality<PluginId, PluginInstanceAsync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceAsync<Ctx>>>>: Send + Sync,
	<Plugins as Cardinality<PluginId, PluginInstanceAsync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceAsync<Ctx>>>>: Cardinality<PluginId, Arc<Mutex<PluginInstanceAsync<Ctx>>>>,
{
	debug_assert_eq!( target.function.kind(), FunctionKind::Method );
	Val::Result( match route_method_async(
		binding,
		scheduler,
		caller,
		path,
		ctx,
		target,
		data,
	).await {
		Ok( val ) => Ok( Some( Box::new( val ))),
		Err( err ) => Err( Some( Box::new( err.into() ))),
	})
}

/// Asynchronously implements a synchronous WIT import without blocking its host thread.
pub(crate) async fn dispatch_all_async_blocking<PluginId, Ctx, Plugins>(
	binding: &Binding<PluginId, Ctx, Plugins, PluginInstanceAsync<Ctx>>,
	scheduler: &AsyncScheduler<Ctx>,
	caller: PluginKey,
	path: ExecutionPathId,
	ctx: StoreContextMut<'_, Ctx>,
	target: &DispatchTarget<'_>,
	data: &[Val],
) -> Val
where
	PluginId: Clone + std::hash::Hash + Eq + Send + Sync + Into<Val> + 'static,
	Ctx: PluginContext,
	Plugins: Cardinality<PluginId, PluginInstanceAsync<Ctx>>,
	<Plugins as Cardinality<PluginId, PluginInstanceAsync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceAsync<Ctx>>>>: Send + Sync,
	<Plugins as Cardinality<PluginId, PluginInstanceAsync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceAsync<Ctx>>>>: Cardinality<PluginId, Arc<Mutex<PluginInstanceAsync<Ctx>>>>,
	<<Plugins as Cardinality<PluginId, PluginInstanceAsync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceAsync<Ctx>>>> as Cardinality<PluginId, Arc<Mutex<PluginInstanceAsync<Ctx>>>>>::Rebind<Val>: Into<Val> + Send,
{
	debug_assert_eq!( target.function.kind(), FunctionKind::Freestanding );
	let ctx = Mutex::new( ctx );
	binding.plugins().map_async(| plugin_id, plugin | async {
		Val::Result( match dispatch_of_async_blocking( DispatchContext::new( scheduler, caller, path ), &ctx, plugin_id, plugin, target, data ).await {
			Ok( val ) => Ok( Some( Box::new( val ))),
			Err( err ) => Err( Some( Box::new( err.into() ))),
		})
	}).await.into()
}

/// Asynchronously implements a synchronous WIT method import.
pub(crate) async fn dispatch_method_async_blocking<PluginId, Ctx, Plugins>(
	binding: &Binding<PluginId, Ctx, Plugins, PluginInstanceAsync<Ctx>>,
	scheduler: &AsyncScheduler<Ctx>,
	caller: PluginKey,
	path: ExecutionPathId,
	ctx: StoreContextMut<'_, Ctx>,
	target: &DispatchTarget<'_>,
	data: &[Val],
) -> Val
where
	PluginId: Clone + std::hash::Hash + Eq + Send + Sync + 'static,
	Ctx: PluginContext,
	Plugins: Cardinality<PluginId, PluginInstanceAsync<Ctx>>,
	<Plugins as Cardinality<PluginId, PluginInstanceAsync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceAsync<Ctx>>>>: Send + Sync,
	<Plugins as Cardinality<PluginId, PluginInstanceAsync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceAsync<Ctx>>>>: Cardinality<PluginId, Arc<Mutex<PluginInstanceAsync<Ctx>>>>,
{
	debug_assert_eq!( target.function.kind(), FunctionKind::Method );
	let ctx = Mutex::new( ctx );
	Val::Result( match route_method_async_blocking(
		binding,
		scheduler,
		caller,
		path,
		&ctx,
		target,
		data,
	).await {
		Ok( val ) => Ok( Some( Box::new( val ))),
		Err( err ) => Err( Some( Box::new( err.into() ))),
	})
}

async fn dispatch_of_async<PluginId, Ctx>(
	dispatch: DispatchContext<'_, Ctx>,
	ctx: &Accessor<Ctx>,
	plugin_id: PluginId,
	plugin: Arc<Mutex<PluginInstanceAsync<Ctx>>>,
	target: &DispatchTarget<'_>,
	data: &[Val],
) -> Result<Val, DispatchError>
where
	PluginId: Clone + std::hash::Hash + Eq + Send + Sync + 'static,
	Ctx: PluginContext,
{
	let instance = plugin.lock().await.clone();
	let result = instance.dispatch_async(
		dispatch,
		target.package_name,
		target.interface_name,
		target.function_name,
		target.function,
		data,
	).await?;

	match target.function.return_kind() {
		ReturnKind::Void | ReturnKind::AssumeNoResources => Ok( result ),
		ReturnKind::MayContainResources => ctx.with(| mut access | {
			let mut store = access.as_context_mut();
			wrap_resources( result, plugin_id, &mut store )
		}),
	}
}

async fn dispatch_of_async_blocking<PluginId, Ctx>(
	dispatch: DispatchContext<'_, Ctx>,
	ctx: &Mutex<StoreContextMut<'_, Ctx>>,
	plugin_id: PluginId,
	plugin: Arc<Mutex<PluginInstanceAsync<Ctx>>>,
	target: &DispatchTarget<'_>,
	data: &[Val],
) -> Result<Val, DispatchError>
where
	PluginId: Clone + std::hash::Hash + Eq + Send + Sync + 'static,
	Ctx: PluginContext,
{
	let instance = plugin.lock().await.clone();
	let result = instance.dispatch_async(
		dispatch,
		target.package_name,
		target.interface_name,
		target.function_name,
		target.function,
		data,
	).await?;

	match target.function.return_kind() {
		ReturnKind::Void | ReturnKind::AssumeNoResources => Ok( result ),
		ReturnKind::MayContainResources => {
			let mut store = ctx.lock().await;
			wrap_resources( result, plugin_id, &mut store )
		}
	}
}

async fn route_method_async<PluginId, Ctx, Plugins>(
	binding: &Binding<PluginId, Ctx, Plugins, PluginInstanceAsync<Ctx>>,
	scheduler: &AsyncScheduler<Ctx>,
	caller: PluginKey,
	path: ExecutionPathId,
	ctx: &Accessor<Ctx>,
	target: &DispatchTarget<'_>,
	data: &[Val],
) -> Result<Val, DispatchError>
where
	PluginId: Clone + std::hash::Hash + Eq + Send + Sync + 'static,
	Ctx: PluginContext,
	Plugins: Cardinality<PluginId, PluginInstanceAsync<Ctx>>,
	<Plugins as Cardinality<PluginId, PluginInstanceAsync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceAsync<Ctx>>>>: Send + Sync,
	<Plugins as Cardinality<PluginId, PluginInstanceAsync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceAsync<Ctx>>>>: Cardinality<PluginId, Arc<Mutex<PluginInstanceAsync<Ctx>>>>,
{
	let handle = match data.first() {
		Some( Val::Resource( handle )) => Ok( *handle ),
		_ => Err( DispatchError::InvalidArgumentList ),
	}?;
	let ( plugin_id, resource_handle ) = ctx.with(| mut access | {
		let mut store = access.as_context_mut();
		let resource = ResourceWrapper::<PluginId>::from_handle( handle, &mut store )?;
		Ok::<_, DispatchError>(( resource.plugin_id.clone(), resource.resource_handle ))
	})?;
	let plugin = binding.plugins().get( &plugin_id )
		.ok_or( DispatchError::InvalidArgumentList )?
		.clone();

	let mut data = Vec::from( data );
	data[0] = Val::Resource( resource_handle );
	dispatch_of_async( DispatchContext::new( scheduler, caller, path ), ctx, plugin_id, plugin, target, &data ).await
}

async fn route_method_async_blocking<PluginId, Ctx, Plugins>(
	binding: &Binding<PluginId, Ctx, Plugins, PluginInstanceAsync<Ctx>>,
	scheduler: &AsyncScheduler<Ctx>,
	caller: PluginKey,
	path: ExecutionPathId,
	ctx: &Mutex<StoreContextMut<'_, Ctx>>,
	target: &DispatchTarget<'_>,
	data: &[Val],
) -> Result<Val, DispatchError>
where
	PluginId: Clone + std::hash::Hash + Eq + Send + Sync + 'static,
	Ctx: PluginContext,
	Plugins: Cardinality<PluginId, PluginInstanceAsync<Ctx>>,
	<Plugins as Cardinality<PluginId, PluginInstanceAsync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceAsync<Ctx>>>>: Send + Sync,
	<Plugins as Cardinality<PluginId, PluginInstanceAsync<Ctx>>>::Rebind<Arc<Mutex<PluginInstanceAsync<Ctx>>>>: Cardinality<PluginId, Arc<Mutex<PluginInstanceAsync<Ctx>>>>,
{
	let handle = match data.first() {
		Some( Val::Resource( handle )) => Ok( *handle ),
		_ => Err( DispatchError::InvalidArgumentList ),
	}?;
	let ( plugin_id, resource_handle ) = {
		let mut store = ctx.lock().await;
		let resource = ResourceWrapper::<PluginId>::from_handle( handle, &mut store )?;
		( resource.plugin_id.clone(), resource.resource_handle )
	};
	let plugin = binding.plugins().get( &plugin_id )
		.ok_or( DispatchError::InvalidArgumentList )?
		.clone();
	let mut data = Vec::from( data );
	data[0] = Val::Resource( resource_handle );
	dispatch_of_async_blocking( DispatchContext::new( scheduler, caller, path ), ctx, plugin_id, plugin, target, &data ).await
}

fn wrap_resources<T, Id>( val: Val, plugin_id: Id, store: &mut StoreContextMut<T> ) -> Result<Val, DispatchError>
where
	T: PluginContext,
	Id: Clone + Send + Sync + 'static,
{
	Ok( match val {
		Val::Bool( _ )
		| Val::S8( _ ) | Val::S16( _ ) | Val::S32( _ ) | Val::S64( _ )
		| Val::U8( _ ) | Val::U16( _ ) | Val::U32( _ ) | Val::U64( _ )
		| Val::Float32( _ ) | Val::Float64( _ )
		| Val::Char( _ )
		| Val::String( _ )
		| Val::Enum( _ )
		| Val::Flags( _ )
		| Val::Variant( _, Option::None )
		| Val::Option( None )
		| Val::Result( Ok( Option::None )) | Val::Result( Err( Option::None )) => val,
		Val::List( list ) => Val::List( list.into_iter().map(| item | wrap_resources( item, plugin_id.clone(), store )).collect::<Result<_,_>>()? ),
		Val::Map( entries ) => Val::Map( entries.into_iter()
			.map(|( key, value )| Ok::<_, DispatchError>((
				wrap_resources( key, plugin_id.clone(), store )?,
				wrap_resources( value, plugin_id.clone(), store )?
			)) )
			.collect::<Result<_,_>>()?
		),
		Val::Record( entries ) => Val::Record( entries.into_iter()
			.map(|( key, value )| Ok::<_, DispatchError>(( key, wrap_resources( value, plugin_id.clone(), store )?)) )
			.collect::<Result<_,_>>()?
		),
		Val::Tuple( list ) => Val::Tuple( list.into_iter().map(| item | wrap_resources( item, plugin_id.clone(), store )).collect::<Result<_,_>>()? ),
		Val::Variant( variant, Some( data_box )) => Val::Variant( variant, Some( Box::new( wrap_resources( *data_box, plugin_id, store )? ))),
		Val::Option( Some( data_box )) => Val::Option( Some( Box::new( wrap_resources( *data_box, plugin_id, store )? ))),
		Val::Result( Ok( Some( data_box ))) => Val::Result( Ok( Some( Box::new( wrap_resources( *data_box, plugin_id, store )? )))),
		Val::Result( Err( Some( data_box ))) => Val::Result( Err( Some( Box::new( wrap_resources( *data_box, plugin_id, store )? )))),
		Val::Resource( handle ) => Val::Resource( ResourceWrapper::new( plugin_id, handle ).attach( store )? ),
		Val::Future( _ ) => return Err( DispatchError::UnsupportedType( "future".to_string() )),
		Val::Stream( _ ) => return Err( DispatchError::UnsupportedType( "stream".to_string() )),
		Val::ErrorContext( _ ) => return Err( DispatchError::UnsupportedType( "error-context".to_string() )),
	})
}

#[cfg(test)]
mod tests { include!( "linker_tests.rs" ); }
