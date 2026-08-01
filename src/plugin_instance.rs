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
	export_asyncness: ExportAsyncness,
	session_locks: Arc<Vec<crate::dispatch_session::SessionLock>>,
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
	Async( Arc<PluginInstanceAsyncInner> ),
	Sync {
		state: Arc<Mutex<PluginState<Ctx>>>,
		interface_remaps: Arc<HashMap<String, Remap>>,
		export_asyncness: Arc<ExportAsyncness>,
		session_locks: Arc<Vec<crate::dispatch_session::SessionLock>>,
	},
}

impl<Ctx> Clone for PluginInstanceAsyncKind<Ctx> {
	fn clone( &self ) -> Self {
		match self {
			Self::Async( inner ) => Self::Async( Arc::clone( inner )),
			Self::Sync { state, interface_remaps, export_asyncness, session_locks } => Self::Sync {
				state: Arc::clone( state ),
				interface_remaps: Arc::clone( interface_remaps ),
				export_asyncness: Arc::clone( export_asyncness ),
				session_locks: Arc::clone( session_locks ),
			},
		}
	}
}

struct PluginInstanceAsyncInner {
	sender: mpsc::UnboundedSender<AsyncRequest>,
	driver: StdMutex<DriverState>,
	interface_remaps: HashMap<String, Remap>,
	export_asyncness: ExportAsyncness,
	session_locks: Arc<Vec<crate::dispatch_session::SessionLock>>,
}

pub(crate) type ExportAsyncness = HashMap<String, HashMap<String, bool>>;
type DriverFuture = BoxFuture<'static, AsyncRuntimeError>;

pub(crate) trait InstanceMetadata {
	fn session_locks( &self ) -> &[crate::dispatch_session::SessionLock];
	fn export_is_async( &self, package_name: &str, interface_name: &str, function_name: &str ) -> bool;
}

enum DriverState {
	Idle( DriverFuture ),
	Attached,
	Failed( AsyncRuntimeError ),
}

struct AsyncRequest {
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

#[derive( Clone, Debug, Error, Eq, PartialEq )]
enum AsyncRuntimeError {
	#[error( "async dispatch session unavailable" )] SessionUnavailable,
	#[error( "plugin dispatch ended without a response" )] MissingResponse,
	#[error( "plugin dispatch driver stopped" )] DriverStopped,
	#[error( "plugin call was cancelled before producing a response" )] CallCancelled,
	#[error( "plugin dispatch request channel closed" )] RequestChannelClosed,
	#[error( "plugin store failed: {0}" )] StoreFailed( String ),
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
				PluginInstanceAsyncKind::Sync { .. } => "<session-managed sync store>",
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
		export_asyncness: ExportAsyncness,
		mut session_locks: Vec<crate::dispatch_session::SessionLock>,
		fuel_limiter: Option<CallLimiter<Ctx>>,
		epoch_limiter: Option<CallLimiter<Ctx>>,
	) -> Self {
		session_locks.push( crate::dispatch_session::new_lock() );
		Self { state: PluginState {
			store,
			instance,
			interface_remaps,
			fuel_limiter,
			epoch_limiter,
		}, export_asyncness, session_locks: Arc::new( crate::dispatch_session::merge_locks( session_locks )) }
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

}

impl<Ctx: PluginContext + 'static> PluginInstanceAsync<Ctx> {
	pub(crate) fn new(
		store: Store<Ctx>,
		instance: Instance,
		interface_remaps: HashMap<String, Remap>,
		export_asyncness: ExportAsyncness,
		mut session_locks: Vec<crate::dispatch_session::SessionLock>,
		fuel_limiter: Option<CallLimiter<Ctx>>,
		epoch_limiter: Option<CallLimiter<Ctx>>,
	) -> Self {
		session_locks.push( crate::dispatch_session::new_lock() );
		let session_locks = Arc::new( crate::dispatch_session::merge_locks( session_locks ));
		let exported_interface_remaps = interface_remaps.clone();
		let mut state = PluginState {
			store,
			instance,
			interface_remaps,
			fuel_limiter,
			epoch_limiter,
		};
		Self {
			kind: PluginInstanceAsyncKind::Async({
				let ( sender, receiver ) = mpsc::unbounded();
				Arc::new( PluginInstanceAsyncInner {
					sender,
					interface_remaps: exported_interface_remaps,
					export_asyncness,
					session_locks,
					driver: StdMutex::new( DriverState::Idle( Box::pin( async move {
						state.run_requests( receiver ).await
					}))),
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
		if let PluginInstanceAsyncKind::Sync { state, .. } = &self.kind {
			return state.lock().await.dispatch(
				package_name,
				interface_name,
				function_name,
				function,
				data,
			);
		}
		let session = crate::dispatch_session::current()
			.ok_or_else(|| runtime_error( AsyncRuntimeError::SessionUnavailable ))?;
		let ( response, result ) = oneshot::channel();
		let request = AsyncRequest {
			caller,
			package_name: package_name.to_string(),
			interface_name: interface_name.to_string(),
			function_name: function_name.to_string(),
			function: function.clone(),
			data: data.to_vec(),
			response,
		};

		let PluginInstanceAsyncKind::Async( inner ) = &self.kind else { unreachable!() };
		inner.enqueue( &session, request )?;
		result.await.map_err(| _ | runtime_error( AsyncRuntimeError::MissingResponse ))?
	}

}

impl<Ctx: PluginContext + 'static> From<PluginInstanceSync<Ctx>> for PluginInstanceAsync<Ctx> {
	/// Makes a synchronous instance usable in an async binding without changing
	/// how its Wasmtime store was instantiated.
	fn from( instance: PluginInstanceSync<Ctx> ) -> Self {
		let PluginInstanceSync { state, export_asyncness, session_locks } = instance;
		let interface_remaps = Arc::new( state.interface_remaps.clone() );
		Self { kind: PluginInstanceAsyncKind::Sync {
			state: Arc::new( Mutex::new( state )),
			interface_remaps,
			export_asyncness: Arc::new( export_asyncness ),
			session_locks,
		}}
	}
}

impl<Ctx: PluginContext + 'static> InstanceMetadata for PluginInstanceSync<Ctx> {
	fn session_locks( &self ) -> &[crate::dispatch_session::SessionLock] { &self.session_locks }

	fn export_is_async( &self, package_name: &str, interface_name: &str, function_name: &str ) -> bool {
		export_is_async(
			&self.export_asyncness,
			&self.state.interface_remaps,
			package_name,
			interface_name,
			function_name,
		)
	}
}

impl<Ctx: PluginContext + 'static> InstanceMetadata for PluginInstanceAsync<Ctx> {
	fn session_locks( &self ) -> &[crate::dispatch_session::SessionLock] {
		match &self.kind {
			PluginInstanceAsyncKind::Async( inner ) => &inner.session_locks,
			PluginInstanceAsyncKind::Sync { session_locks, .. } => session_locks,
		}
	}

	fn export_is_async( &self, package_name: &str, interface_name: &str, function_name: &str ) -> bool {
		match &self.kind {
			PluginInstanceAsyncKind::Async( inner ) => export_is_async(
				&inner.export_asyncness,
				&inner.interface_remaps,
				package_name,
				interface_name,
				function_name,
			),
			PluginInstanceAsyncKind::Sync { interface_remaps, export_asyncness, .. } => export_is_async(
				export_asyncness,
				interface_remaps,
				package_name,
				interface_name,
				function_name,
			),
		}
	}
}

impl PluginInstanceAsyncInner {
	fn enqueue(
		self: &Arc<Self>,
		session: &Arc<crate::dispatch_session::SessionShared>,
		request: AsyncRequest,
	) -> Result<(), DispatchError> {
		let mut driver = self.driver.lock().unwrap_or_else( std::sync::PoisonError::into_inner );
		let future = match std::mem::replace( &mut *driver, DriverState::Attached ) {
			DriverState::Idle( future ) => Some( future ),
			DriverState::Attached => None,
			DriverState::Failed( error ) => {
				*driver = DriverState::Failed( error.clone() );
				return Err( runtime_error( error ));
			}
		};
		if self.sender.unbounded_send( request ).is_err() {
			*driver = DriverState::Failed( AsyncRuntimeError::DriverStopped );
			return Err( runtime_error( AsyncRuntimeError::DriverStopped ));
		}
		drop( driver );
		if let Some( future ) = future {
				session.spawn( Box::pin( AttachedDriver {
					inner: Arc::clone( self ),
					future: Some( future ),
				}));
		}
		Ok(())
	}

	fn release( &self, future: DriverFuture ) {
		*self.driver.lock().unwrap_or_else( std::sync::PoisonError::into_inner ) = DriverState::Idle( future );
	}

	fn fail( &self, error: AsyncRuntimeError ) {
		*self.driver.lock().unwrap_or_else( std::sync::PoisonError::into_inner ) = DriverState::Failed( error );
	}
}

struct AttachedDriver {
	inner: Arc<PluginInstanceAsyncInner>,
	future: Option<DriverFuture>,
}

impl Future for AttachedDriver {
	type Output = ();

	fn poll( mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_> ) -> std::task::Poll<()> {
		let result = match self.future.as_mut() {
			Some( future ) => std::pin::Pin::new( future ).poll( cx ),
			None => return std::task::Poll::Ready(()),
		};
		match result {
			std::task::Poll::Pending => std::task::Poll::Pending,
			std::task::Poll::Ready( error ) => {
				self.future.take();
				self.inner.fail( error );
				std::task::Poll::Ready(())
			}
		}
	}
}

impl Drop for AttachedDriver {
	fn drop( &mut self ) {
		if let Some( future ) = self.future.take() { self.inner.release( future ); }
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

	async fn run_requests( &mut self, mut receiver: mpsc::UnboundedReceiver<AsyncRequest> ) -> AsyncRuntimeError {
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
			return AsyncRuntimeError::RequestChannelClosed;
		}

		let instance = self.instance;
		let interface_remaps = &self.interface_remaps;
		let run_result = self.store.run_concurrent( async | accessor | {
			let mut queues = RequestQueues::default();
			let mut calls = FuturesUnordered::<BoxFuture<'_, ()>>::new();
			loop {
				if calls.is_empty() {
					let Some( request ) = receiver.next().await else { return };
					queues.push( request );
					while let Ok( request ) = receiver.try_recv() { queues.push( request ); }
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
							queues.push( request );
							while let Ok( request ) = receiver.try_recv() { queues.push( request ); }
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
			while let Ok( request ) = receiver.try_recv() {
				let _ = request.response.send( Err( runtime_error( AsyncRuntimeError::StoreFailed( error.to_string() ))));
			}
			return AsyncRuntimeError::StoreFailed( error.to_string() );
		}
		AsyncRuntimeError::DriverStopped
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
		resolve_export( &self.interface_remaps, package_name, interface_name, function_name )
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
		self.send( Err( runtime_error( AsyncRuntimeError::CallCancelled )));
	}
}

fn runtime_error( error: AsyncRuntimeError ) -> DispatchError {
	DispatchError::RuntimeException( error.into() )
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

fn export_is_async(
	export_asyncness: &ExportAsyncness,
	interface_remaps: &HashMap<String, Remap>,
	package_name: &str,
	interface_name: &str,
	function_name: &str,
) -> bool {
	let ( interface_path, function_name ) = resolve_export(
		interface_remaps,
		package_name,
		interface_name,
		function_name,
	);
	export_asyncness.get( &interface_path )
		.and_then(| functions | functions.get( &function_name ))
		.copied()
		.unwrap_or( false )
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
