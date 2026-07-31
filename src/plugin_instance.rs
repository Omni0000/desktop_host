use std::collections::{ HashMap, VecDeque };
use std::future::Future ;
use std::sync::{ Arc, Mutex as StdMutex };
use futures::channel::{ mpsc, oneshot };
use futures::future::{ BoxFuture, FutureExt };
use futures::lock::Mutex ;
use futures::stream::{ FuturesUnordered, StreamExt };
use thiserror::Error ;
use wasmtime::component::{ Accessor, Func, Instance, Val };
use wasmtime::{ AsContextMut, Store };

use crate::{ Function, PluginContext, Remap, ReturnKind };
use crate::resource_wrapper::{ ResourceCreationError, ResourceReceiveError };

type CallLimiter<Ctx> = Box<dyn FnMut( &mut Store<Ctx>, &str, &str, &Function ) -> u64 + Send>;


/// A synchronously instantiated plugin, ready for synchronous dispatch.
///
/// Created by calling [`Plugin::instantiate`]( crate::Plugin::instantiate ),
/// or [`Plugin::link`]( crate::Plugin::link ).
pub struct PluginInstanceSync<Ctx: 'static> {
	state: PluginState<Ctx>,
}

/// An asynchronously instantiated plugin, ready for asynchronous dispatch.
///
/// Created by calling [`Plugin::instantiate_async`]( crate::Plugin::instantiate_async )
/// or [`Plugin::link_async`]( crate::Plugin::link_async ). Its Wasmtime [`Store`]
/// is driven cooperatively by the future returned from asynchronous dispatch.
pub struct PluginInstanceAsync<Ctx: 'static> {
	kind: PluginInstanceAsyncKind<Ctx>,
}

impl<Ctx> Clone for PluginInstanceAsync<Ctx> {
	fn clone( &self ) -> Self { Self { kind: self.kind.clone() } }
}

enum PluginInstanceAsyncKind<Ctx: 'static> {
	Async( Arc<PluginInstanceAsyncInner<Ctx>> ),
	Sync( Arc<Mutex<PluginInstanceSync<Ctx>>> ),
}

impl<Ctx> Clone for PluginInstanceAsyncKind<Ctx> {
	fn clone( &self ) -> Self {
		match self {
			Self::Async( inner ) => Self::Async( Arc::clone( inner )),
			Self::Sync( instance ) => Self::Sync( Arc::clone( instance )),
		}
	}
}

struct PluginInstanceAsyncInner<Ctx: 'static> {
	sender: mpsc::UnboundedSender<AsyncRequest>,
	driver: StdMutex<DriverCoordinator<Ctx>>,
}

type DriverFuture = BoxFuture<'static, Result<(), String>>;

struct DriverCoordinator<Ctx: 'static> {
	state: Option<PluginState<Ctx>>,
	receiver: Option<mpsc::UnboundedReceiver<AsyncRequest>>,
	future: Option<DriverFuture>,
	active_session: Option<u64>,
	waiters: VecDeque<SessionWaiter>,
	terminal_error: Option<String>,
}

struct SessionWaiter {
	id: u64,
	session: Arc<crate::dispatch_session::SessionShared>,
	requests: Vec<AsyncRequest>,
}

struct AsyncRequest {
	session: u64,
	caller: u64,
	package_name: String,
	interface_name: String,
	function_name: String,
	function: Function,
	data: Vec<Val>,
	response: oneshot::Sender<Result<Val, DispatchError>>,
}

struct PluginState<Ctx: 'static> {
	store: Store<Ctx>,
	instance: Instance,
	interface_remaps: HashMap<String, Remap>,
	fuel_limiter: Option<CallLimiter<Ctx>>,
	epoch_limiter: Option<CallLimiter<Ctx>>,
}

impl<Ctx: std::fmt::Debug + 'static> std::fmt::Debug for PluginInstanceSync<Ctx> {
	fn fmt( &self, f: &mut std::fmt::Formatter<'_> ) -> std::result::Result<(), std::fmt::Error> {
		f.debug_struct( "PluginInstanceSync" )
			.field( "data", &self.state.store.data() )
			.field( "store", &self.state.store )
			.field( "interface_remaps", &self.state.interface_remaps )
			.field( "fuel_limiter", &self.state.fuel_limiter.as_ref().map(| _ | "<closure>" ))
			.field( "epoch_limiter", &self.state.epoch_limiter.as_ref().map(| _ | "<closure>" ))
			.finish_non_exhaustive()
	}
}

impl<Ctx: 'static> std::fmt::Debug for PluginInstanceAsync<Ctx> {
	fn fmt( &self, f: &mut std::fmt::Formatter<'_> ) -> std::result::Result<(), std::fmt::Error> {
		f.debug_struct( "PluginInstanceAsync" )
			.field( "state", &match &self.kind {
				PluginInstanceAsyncKind::Async( _ ) => "<session-managed async store>",
				PluginInstanceAsyncKind::Sync( _ ) => "<session-managed sync store>",
			})
			.finish_non_exhaustive()
	}
}

/// Errors that can occur when dispatching a function call to plugins.
///
/// Returned inside a cardinality wrapper from
/// [`Binding::dispatch`]( crate::binding::Binding::dispatch )
/// when a function call fails at runtime.
#[derive( Error, Debug )]
pub enum DispatchError {
	/// Failed to acquire lock on plugin instance (another call is in progress).
	#[error( "Lock Rejected" )] LockRejected,
	/// The specified interface path doesn't match any known interface.
	#[error( "Invalid Interface Path: {0}" )] InvalidInterfacePath( String ),
	/// The specified function doesn't exist on the interface.
	#[error( "Invalid Function: {0}" )] InvalidFunction( String ),
	/// Function was expected to return a value but didn't.
	#[error( "Missing Response" )] MissingResponse,
	/// The WASM function threw an exception during execution.
	#[error( "Runtime Exception" )] RuntimeException( wasmtime::Error ),
	/// The provided arguments don't match the function signature.
	#[error( "Invalid Argument List" )] InvalidArgumentList,
	/// Async types (`Future`, `Stream`, `ErrorContext`) are not yet supported for cross-plugin transfer.
	#[error( "Unsupported type: {0}" )] UnsupportedType( String ),
	/// Failed to create a resource handle for cross-plugin transfer.
	#[error( "Resource Create Error: {0}" )] ResourceCreationError( #[from] ResourceCreationError ),
	/// Failed to receive a resource handle from another plugin.
	#[error( "Resource Receive Error: {0}" )] ResourceReceiveError( #[from] ResourceReceiveError ),
}

impl From<DispatchError> for Val {
	fn from( error: DispatchError ) -> Val { match error {
		DispatchError::LockRejected => Val::Variant( "lock-rejected".to_string(), None ),
		DispatchError::InvalidInterfacePath( package ) => Val::Variant( "invalid-interface-path".to_string(), Some( Box::new( Val::String( package )))),
		DispatchError::InvalidFunction( function ) => Val::Variant( "invalid-function".to_string(), Some( Box::new( Val::String( function )))),
		DispatchError::MissingResponse => Val::Variant( "missing-response".to_string(), None ),
		DispatchError::RuntimeException( exception ) => Val::Variant( "runtime-exception".to_string(), Some( Box::new( Val::String( exception.to_string() )))),
		DispatchError::InvalidArgumentList => Val::Variant( "invalid-argument-list".to_string(), None ),
		DispatchError::UnsupportedType( name ) => Val::Variant( "unsupported-type".to_string(), Some( Box::new( Val::String( name )))),
		DispatchError::ResourceCreationError( err ) => err.into(),
		DispatchError::ResourceReceiveError( err ) => err.into(),
	}}
}

impl<Ctx: PluginContext + 'static> PluginInstanceSync<Ctx> {
	pub(crate) fn new_sync(
		store: Store<Ctx>,
		instance: Instance,
		interface_remaps: HashMap<String, Remap>,
		fuel_limiter: Option<CallLimiter<Ctx>>,
		epoch_limiter: Option<CallLimiter<Ctx>>,
	) -> Self {
		Self { state: PluginState {
			store,
			instance,
			interface_remaps,
			fuel_limiter,
			epoch_limiter,
		}}
	}

	pub(crate) fn dispatch(
		&mut self,
		package_name: &str,
		interface_name: &str,
		function_name: &str,
		function: &Function,
		data: &[Val],
	) -> Result<Val, DispatchError> {
		self.state.dispatch( package_name, interface_name, function_name, function, data )
	}

	pub(crate) fn export_is_async(
		&mut self,
		package_name: &str,
		interface_name: &str,
		function_name: &str,
	) -> bool {
		self.state.export_is_async( package_name, interface_name, function_name )
	}
}

impl<Ctx: PluginContext + 'static> PluginInstanceAsync<Ctx> {
	pub(crate) fn new(
		store: Store<Ctx>,
		instance: Instance,
		interface_remaps: HashMap<String, Remap>,
		fuel_limiter: Option<CallLimiter<Ctx>>,
		epoch_limiter: Option<CallLimiter<Ctx>>,
	) -> Self {
		Self {
			kind: PluginInstanceAsyncKind::Async({
				let ( sender, receiver ) = mpsc::unbounded();
				Arc::new( PluginInstanceAsyncInner {
					sender,
					driver: StdMutex::new( DriverCoordinator {
						state: Some( PluginState {
							store,
							instance,
							interface_remaps,
							fuel_limiter,
							epoch_limiter,
						}),
						receiver: Some( receiver ),
						future: None,
						active_session: None,
						waiters: VecDeque::new(),
						terminal_error: None,
					}),
				})
			}),
		}
	}

	pub(crate) async fn dispatch_async(
		&self,
		package_name: &str,
		interface_name: &str,
		function_name: &str,
		function: &Function,
		data: &[Val],
	) -> Result<Val, DispatchError> {
		self.dispatch_async_from( 0, package_name, interface_name, function_name, function, data ).await
	}

	pub(crate) async fn dispatch_async_from(
		&self,
		caller: u64,
		package_name: &str,
		interface_name: &str,
		function_name: &str,
		function: &Function,
		data: &[Val],
	) -> Result<Val, DispatchError> {
		ensure_supported_values( data )?;
		if let PluginInstanceAsyncKind::Sync( instance ) = &self.kind {
			return instance.lock().await.dispatch(
				package_name,
				interface_name,
				function_name,
				function,
				data,
			);
		}
		let session = crate::dispatch_session::current()
			.ok_or_else(|| runtime_error( "async dispatch session unavailable" ))?;
		let ( response, result ) = oneshot::channel();
		let request = AsyncRequest {
			session: session.id(),
			caller,
			package_name: package_name.to_string(),
			interface_name: interface_name.to_string(),
			function_name: function_name.to_string(),
			function: function.clone(),
			data: data.to_vec(),
			response,
		};

		let PluginInstanceAsyncKind::Async( inner ) = &self.kind else { unreachable!() };
		inner.enqueue( session, request )?;
		result.await.map_err(| _ |
			DispatchError::RuntimeException( wasmtime::Error::msg( "plugin dispatch ended without a response" ))
		)?
	}

	pub(crate) fn export_is_async(
		&self,
		package_name: &str,
		interface_name: &str,
		function_name: &str,
	) -> Result<bool, wasmtime::Error> {
		match &self.kind {
			PluginInstanceAsyncKind::Async( inner ) => match inner.driver.lock() {
				Ok( mut driver ) => Ok( driver.state.as_mut()
					.ok_or_else(|| wasmtime::Error::msg( "plugin is busy during link-time export inspection" ))?
					.export_is_async( package_name, interface_name, function_name )),
				Err( poisoned ) => Ok( poisoned.into_inner().state.as_mut()
					.ok_or_else(|| wasmtime::Error::msg( "plugin is busy during link-time export inspection" ))?
					.export_is_async( package_name, interface_name, function_name )),
			},
			PluginInstanceAsyncKind::Sync( instance ) => Ok( instance.try_lock()
				.ok_or_else(|| wasmtime::Error::msg( "plugin is busy during link-time export inspection" ))?
				.state.export_is_async( package_name, interface_name, function_name )),
		}
	}

}

impl<Ctx: PluginContext + 'static> From<PluginInstanceSync<Ctx>> for PluginInstanceAsync<Ctx> {
	/// Makes a synchronous instance usable in an async binding without changing
	/// how its Wasmtime store was instantiated.
	fn from( instance: PluginInstanceSync<Ctx> ) -> Self {
		Self { kind: PluginInstanceAsyncKind::Sync( Arc::new( Mutex::new( instance ))) }
	}
}

impl<Ctx: PluginContext + 'static> PluginInstanceAsyncInner<Ctx> {
	fn enqueue(
		self: &Arc<Self>,
		session: Arc<crate::dispatch_session::SessionShared>,
		request: AsyncRequest,
	) -> Result<(), DispatchError> {
		let session_id = session.id();
		let mut driver = match self.driver.lock() {
			Ok( driver ) => driver,
			Err( poisoned ) => poisoned.into_inner(),
		};
		if let Some( error ) = &driver.terminal_error {
			return Err( runtime_error( error ));
		}

		match driver.active_session {
			Some( active ) if active == session_id => {
				return self.sender.unbounded_send( request )
					.map_err(| _ | runtime_error( "plugin dispatch driver stopped" ));
			}
			Some( _ ) => {
				match driver.waiters.iter_mut().find(| waiter | waiter.id == session_id ) {
					Some( waiter ) => waiter.requests.push( request ),
					None => driver.waiters.push_back( SessionWaiter {
						id: session_id,
						session,
						requests: vec![ request ],
					}),
				}
				return Ok(());
			}
			None => {}
		}

		let future = if let Some( future ) = driver.future.take() {
			future
		} else {
				let mut state = driver.state.take()
					.ok_or_else(|| runtime_error( "plugin dispatch state unavailable" ))?;
				let receiver = driver.receiver.take()
					.ok_or_else(|| runtime_error( "plugin dispatch receiver unavailable" ))?;
				Box::pin( async move { state.run_requests( receiver ).await })
		};
		driver.active_session = Some( session_id );
		self.sender.unbounded_send( request )
			.map_err(| _ | runtime_error( "plugin dispatch driver stopped" ))?;
		drop( driver );
		session.spawn( Box::pin( AttachedDriver {
			inner: Arc::clone( self ),
			session_id,
			future: Some( future ),
			completed: false,
		}));
		Ok(())
	}

	fn release( self: &Arc<Self>, session_id: u64, future: DriverFuture ) {
		let mut driver = match self.driver.lock() {
			Ok( driver ) => driver,
			Err( poisoned ) => poisoned.into_inner(),
		};
		if driver.active_session != Some( session_id ) {
			driver.future = Some( future );
			return;
		}

		let waiter = loop {
			match driver.waiters.pop_front() {
				Some( waiter ) if waiter.session.is_cancelled() => {
					for request in waiter.requests {
						let _ = request.response.send( Err( runtime_error( "dispatch session was cancelled" )));
					}
				}
				Some( waiter ) => break waiter,
				None => {
					driver.active_session = None;
					driver.future = Some( future );
					return;
				}
			}
		};
		driver.active_session = Some( waiter.id );
		for request in waiter.requests {
			if let Err( error ) = self.sender.unbounded_send( request ) {
				let _ = error.into_inner().response.send( Err( runtime_error( "plugin dispatch driver stopped" )));
			}
		}
		drop( driver );
		waiter.session.spawn( Box::pin( AttachedDriver {
			inner: Arc::clone( self ),
			session_id: waiter.id,
			future: Some( future ),
			completed: false,
		}));
	}

	fn fail( &self, session_id: u64, error: &str ) {
		let mut driver = match self.driver.lock() {
			Ok( driver ) => driver,
			Err( poisoned ) => poisoned.into_inner(),
		};
		if driver.active_session != Some( session_id ) { return; }
		driver.active_session = None;
		driver.terminal_error = Some( error.to_string() );
		for waiter in driver.waiters.drain( .. ) {
			for request in waiter.requests {
				let _ = request.response.send( Err( runtime_error( error )));
			}
		}
	}
}

struct AttachedDriver<Ctx: PluginContext + 'static> {
	inner: Arc<PluginInstanceAsyncInner<Ctx>>,
	session_id: u64,
	future: Option<DriverFuture>,
	completed: bool,
}

impl<Ctx: PluginContext + 'static> Future for AttachedDriver<Ctx> {
	type Output = ();

	fn poll( mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_> ) -> std::task::Poll<()> {
		let result = match self.future.as_mut() {
			Some( future ) => std::pin::Pin::new( future ).poll( cx ),
			None => return std::task::Poll::Ready(()),
		};
		match result {
			std::task::Poll::Pending => std::task::Poll::Pending,
			std::task::Poll::Ready( result ) => {
				self.future.take();
				self.completed = true;
				let error = result.err().unwrap_or_else(|| "plugin dispatch driver stopped".to_string());
				self.inner.fail( self.session_id, &error );
				std::task::Poll::Ready(())
			}
		}
	}
}

impl<Ctx: PluginContext> Drop for AttachedDriver<Ctx> {
	fn drop( &mut self ) {
		if !self.completed {
			if let Some( future ) = self.future.take() { self.inner.release( self.session_id, future ); }
		}
	}
}

impl<Ctx: PluginContext + 'static> PluginState<Ctx> {
	const PLACEHOLDER_VAL: Val = Val::Option( None );
	const VOID_RETURN_VAL: Val = Val::Option( None );

	fn dispatch(
		&mut self,
		package_name: &str,
		interface_name: &str,
		function_name: &str,
		function: &Function,
		data: &[Val],
	) -> Result<Val, DispatchError> {
		ensure_supported_values( data )?;
		let mut buffer = self.prepare_call( package_name, interface_name, function_name, function )?;
		let ( exported_interface_path, exported_function_name ) = self.resolve_export( package_name, interface_name, function_name );
		let func = self.function( &exported_interface_path, &exported_function_name )?;
		let call_result = func.call( &mut self.store, data, &mut buffer );
		Self::finish_call( function, buffer, call_result )
	}

	async fn dispatch_async(
		&mut self,
		package_name: &str,
		interface_name: &str,
		function_name: &str,
		function: &Function,
		data: &[Val],
	) -> Result<Val, DispatchError> {
		ensure_supported_values( data )?;
		let mut buffer = self.prepare_call( package_name, interface_name, function_name, function )?;
		let ( exported_interface_path, exported_function_name ) = self.resolve_export( package_name, interface_name, function_name );
		let func = self.function( &exported_interface_path, &exported_function_name )?;
		let call_result = func.call_async( &mut self.store, data, &mut buffer ).await;
		Self::finish_call( function, buffer, call_result )
	}

	async fn run_requests( &mut self, mut receiver: mpsc::UnboundedReceiver<AsyncRequest> ) -> Result<(), String> {
		if self.fuel_limiter.is_some() || self.epoch_limiter.is_some() {
			while let Some( request ) = receiver.next().await {
				let result = self.dispatch_async(
					&request.package_name,
					&request.interface_name,
					&request.function_name,
					&request.function,
					&request.data,
				).await;
				let _ = request.response.send( result );
			}
			return Err( "plugin dispatch request channel closed".to_string() );
		}

		let instance = self.instance;
		let interface_remaps = &self.interface_remaps;
		let run_result = self.store.run_concurrent( async | accessor | {
			let mut queues = RequestQueues::default();
			let mut deferred = VecDeque::new();
			let mut calls = FuturesUnordered::<BoxFuture<'_, ()>>::new();
			let mut active_session = None;
			loop {
				if calls.is_empty() {
					let request = match deferred.pop_front() {
						Some( request ) => request,
						None => match receiver.next().await {
							Some( request ) => request,
							None => return,
						},
					};
					active_session = Some( request.session );
					queues.push( request );
					while let Ok( request ) = receiver.try_recv() {
						if Some( request.session ) == active_session { queues.push( request ); }
						else { deferred.push_back( request ); }
					}
					while let Some( request ) = queues.pop() {
						start_concurrent_call( accessor, instance, interface_remaps, request, &mut calls );
					}
					continue;
				}

				let request = receiver.next().fuse();
				let completed = calls.next().fuse();
				futures::pin_mut!( request, completed );
				futures::select_biased! {
					request = request => match request {
						Some( request ) => {
							if Some( request.session ) == active_session { queues.push( request ); }
							else { deferred.push_back( request ); }
							while let Ok( request ) = receiver.try_recv() {
								if Some( request.session ) == active_session { queues.push( request ); }
								else { deferred.push_back( request ); }
							}
							while let Some( request ) = queues.pop() {
								start_concurrent_call( accessor, instance, interface_remaps, request, &mut calls );
							}
						}
						None => while calls.next().await.is_some() {},
					},
					_ = completed => {},
				}
			}
		}).await;

		if let Err( error ) = run_result {
			let message = error.to_string();
			while let Ok( request ) = receiver.try_recv() {
				let _ = request.response.send( Err( runtime_error( &message )));
			}
			return Err( message );
		}
		Err( "plugin dispatch driver stopped".to_string() )
	}

	fn prepare_call(
		&mut self,
		package_name: &str,
		interface_name: &str,
		function_name: &str,
		function: &Function,
	) -> Result<Vec<Val>, DispatchError> {
		let canonical_interface_path = format!( "{}/{}", package_name, interface_name );
		if let Some( mut limiter ) = self.fuel_limiter.take() {
			let fuel = limiter( &mut self.store, &canonical_interface_path, function_name, function );
			self.fuel_limiter = Some( limiter );
			self.store.set_fuel( fuel ).map_err( DispatchError::RuntimeException )?;
		}
		if let Some( mut limiter ) = self.epoch_limiter.take() {
			let ticks = limiter( &mut self.store, &canonical_interface_path, function_name, function );
			self.epoch_limiter = Some( limiter );
			self.store.set_epoch_deadline( ticks );
		}
		Ok( match function.return_kind() != ReturnKind::Void {
			true => vec![ Self::PLACEHOLDER_VAL ],
			false => Vec::with_capacity( 0 ),
		})
	}

	fn function( &mut self, interface_path: &str, function_name: &str ) -> Result<wasmtime::component::Func, DispatchError> {
		let interface_index = self.instance
			.get_export_index( &mut self.store, None, interface_path )
			.ok_or_else(|| DispatchError::InvalidInterfacePath( interface_path.to_string() ))?;
		let func_index = self.instance
			.get_export_index( &mut self.store, Some( &interface_index ), function_name )
			.ok_or_else(|| DispatchError::InvalidFunction( format!( "{interface_path}:{function_name}" )))?;
		self.instance
			.get_func( &mut self.store, func_index )
			.ok_or_else(|| DispatchError::InvalidFunction( format!( "{interface_path}:{function_name}" )))
	}

	fn finish_call(
		function: &Function,
		mut buffer: Vec<Val>,
		call_result: Result<(), wasmtime::Error>,
	) -> Result<Val, DispatchError> {
		call_result.map_err( DispatchError::RuntimeException )?;
		let result = match function.return_kind() != ReturnKind::Void {
			true => buffer.pop().ok_or( DispatchError::MissingResponse )?,
			false => Self::VOID_RETURN_VAL,
		};
		ensure_supported_value( &result )?;
		Ok( result )
	}

	fn resolve_export( &self, package_name: &str, interface_name: &str, function_name: &str ) -> (String, String) {
		match self.interface_remaps.get( interface_name ) {
			Some( remap ) => (
				format!( "{}/{}", package_name, remap.interface_name( interface_name )),
				remap.item_name( function_name ).to_string(),
			),
			None => (
				format!( "{}/{}", package_name, interface_name ),
				function_name.to_string(),
			),
		}
	}

	fn export_is_async(
		&mut self,
		package_name: &str,
		interface_name: &str,
		function_name: &str,
	) -> bool {
		let ( interface_path, function_name ) = self.resolve_export( package_name, interface_name, function_name );
		let Some( interface_index ) = self.instance.get_export_index( &mut self.store, None, &interface_path ) else {
			return false;
		};
		let Some( function_index ) = self.instance.get_export_index( &mut self.store, Some( &interface_index ), &function_name ) else {
			return false;
		};
		let Some( function ) = self.instance.get_func( &mut self.store, function_index ) else {
			return false;
		};
		function.ty( &self.store ).async_()
	}

}

#[derive( Default )]
struct RequestQueues {
	callers: VecDeque<u64>,
	requests: HashMap<u64, VecDeque<AsyncRequest>>,
}

impl RequestQueues {
	fn push( &mut self, request: AsyncRequest ) {
		let caller = request.caller;
		let requests = self.requests.entry( caller ).or_default();
		if requests.is_empty() { self.callers.push_back( caller ); }
		requests.push_back( request );
	}

	fn pop( &mut self ) -> Option<AsyncRequest> {
		let caller = self.callers.pop_front()?;
		let requests = self.requests.get_mut( &caller )?;
		let request = requests.pop_front();
		if requests.is_empty() {
			self.requests.remove( &caller );
		} else {
			self.callers.push_back( caller );
		}
		request
	}
}

fn start_concurrent_call<'a, Ctx: PluginContext + 'static>(
	accessor: &'a Accessor<Ctx>,
	instance: Instance,
	interface_remaps: &HashMap<String, Remap>,
	request: AsyncRequest,
	calls: &mut FuturesUnordered<BoxFuture<'a, ()>>,
) {
	let ( interface_path, function_name ) = resolve_export(
		interface_remaps,
		&request.package_name,
		&request.interface_name,
		&request.function_name,
	);
	let function = accessor.with(| mut access | {
		let mut store = access.as_context_mut();
		let interface_index = instance.get_export_index( &mut store, None, &interface_path )
			.ok_or_else(|| DispatchError::InvalidInterfacePath( interface_path.clone() ))?;
		let function_index = instance.get_export_index( &mut store, Some( &interface_index ), &function_name )
			.ok_or_else(|| DispatchError::InvalidFunction( format!( "{interface_path}:{function_name}" )))?;
		instance.get_func( &mut store, function_index )
			.ok_or_else(|| DispatchError::InvalidFunction( format!( "{interface_path}:{function_name}" )))
	});
	let function = match function {
		Ok( function ) => function,
		Err( error ) => {
			let _ = request.response.send( Err( error ));
			return;
		}
	};
	let results = match request.function.return_kind() != ReturnKind::Void {
		true => vec![ PluginState::<Ctx>::PLACEHOLDER_VAL ],
		false => Vec::new(),
	};
	calls.push( concurrent_call( accessor, function, request, results ));
}

fn concurrent_call<Ctx: PluginContext + 'static>(
	accessor: &Accessor<Ctx>,
	function: Func,
	request: AsyncRequest,
	mut results: Vec<Val>,
) -> BoxFuture<'_, ()> {
	Box::pin( async move {
		let mut response = ResponseGuard::new( request.response );
		let call_result = function.call_concurrent( accessor, &request.data, &mut results ).await;
		response.send( PluginState::<Ctx>::finish_call( &request.function, results, call_result ));
	})
}

struct ResponseGuard {
	response: Option<oneshot::Sender<Result<Val, DispatchError>>>,
}

impl ResponseGuard {
	fn new( response: oneshot::Sender<Result<Val, DispatchError>> ) -> Self {
		Self { response: Some( response ) }
	}

	fn send( &mut self, result: Result<Val, DispatchError> ) {
		if let Some( response ) = self.response.take() { let _ = response.send( result ); }
	}
}

impl Drop for ResponseGuard {
	fn drop( &mut self ) {
		self.send( Err( runtime_error( "plugin call was cancelled before producing a response" )));
	}
}

fn runtime_error( message: &str ) -> DispatchError {
	DispatchError::RuntimeException( wasmtime::Error::msg( message.to_string() ))
}

fn resolve_export(
	interface_remaps: &HashMap<String, Remap>,
	package_name: &str,
	interface_name: &str,
	function_name: &str,
) -> (String, String) {
	match interface_remaps.get( interface_name ) {
		Some( remap ) => (
			format!( "{}/{}", package_name, remap.interface_name( interface_name )),
			remap.item_name( function_name ).to_string(),
		),
		None => ( format!( "{package_name}/{interface_name}" ), function_name.to_string() ),
	}
}

fn ensure_supported_values( values: &[Val] ) -> Result<(), DispatchError> {
	values.iter().try_for_each( ensure_supported_value )
}

fn ensure_supported_value( value: &Val ) -> Result<(), DispatchError> {
	match value {
		Val::List( values ) | Val::Tuple( values ) => ensure_supported_values( values ),
		Val::Map( values ) => values.iter().try_for_each(|( key, value )| {
			ensure_supported_value( key )?;
			ensure_supported_value( value )
		}),
		Val::Record( values ) => values.iter().try_for_each(|( _, value )| ensure_supported_value( value )),
		Val::Variant( _, Some( value ))
		| Val::Option( Some( value ))
		| Val::Result( Ok( Some( value )))
		| Val::Result( Err( Some( value ))) => ensure_supported_value( value ),
		Val::Future( _ ) => Err( DispatchError::UnsupportedType( "future".to_string() )),
		Val::Stream( _ ) => Err( DispatchError::UnsupportedType( "stream".to_string() )),
		Val::ErrorContext( _ ) => Err( DispatchError::UnsupportedType( "error-context".to_string() )),
		_ => Ok(()),
	}
}

#[cfg(test)] mod tests { include!( "plugin_instance_tests.rs" ); }
