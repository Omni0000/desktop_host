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
use crate::async_scheduler::{ ActiveCall, ActiveCallRecord, ActiveCalls, AsyncScheduler, DispatchContext, PluginKey };
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
	instance: PluginInstanceAny<Ctx>,
}

enum PluginInstanceAny<Ctx: 'static> {
	Async( Arc<AsyncInstanceInner<Ctx>> ),
	Sync( Arc<SyncInstanceInner<Ctx>> ),
}

struct SyncInstanceInner<Ctx: 'static> {
	state: Mutex<PluginState<Ctx>>,
}

pub(crate) struct PluginDispatchHandle<Ctx: 'static> {
	instance: PluginInstanceAny<Ctx>,
}

impl<Ctx> Clone for PluginInstanceAny<Ctx> {
	fn clone( &self ) -> Self {
		match self {
			Self::Async( inner ) => Self::Async( Arc::clone( inner )),
			Self::Sync( inner ) => Self::Sync( Arc::clone( inner )),
		}
	}
}

impl<Ctx> Clone for PluginDispatchHandle<Ctx> {
	fn clone( &self ) -> Self { Self { instance: self.instance.clone() } }
}

struct AsyncInstanceInner<Ctx: 'static> {
	sender: mpsc::UnboundedSender<DriverMessage<Ctx>>,
	driver: StdMutex<DriverState<Ctx>>,
}

pub(crate) struct AsyncLinkage<Ctx: 'static> {
	active_calls: ActiveCalls<Ctx>,
}

impl<Ctx: 'static> AsyncLinkage<Ctx> {
	pub(crate) fn new() -> Self {
		Self {
			active_calls: Arc::new( StdMutex::new( HashMap::new() )),
		}
	}

	pub(crate) fn active_calls( &self ) -> &ActiveCalls<Ctx> { &self.active_calls }
}

type DriverFuture = BoxFuture<'static, AsyncRuntimeError>;

enum DriverState<Ctx: 'static> {
	Idle( DriverFuture ),
	Attached {
		owner: usize,
		waiting: VecDeque<PendingSession<Ctx>>,
	},
	Failed( AsyncRuntimeError ),
}

struct PendingSession<Ctx: 'static> {
	scheduler: AsyncScheduler<Ctx>,
	requests: VecDeque<AsyncRequest<Ctx>>,
}

pub(crate) struct AsyncRequest<Ctx: 'static> {
	package_name: String,
	interface_name: String,
	function_name: String,
	function: Function,
	data: Vec<Val>,
	response: Option<oneshot::Sender<Result<Val, DispatchError>>>,
	active: ActiveCall<Ctx>,
}

enum DriverMessage<Ctx: 'static> {
	Call( AsyncRequest<Ctx> ),
}

impl<Ctx: 'static> AsyncRequest<Ctx> {
	fn respond( mut self, result: Result<Val, DispatchError> ) {
		if let Some( response ) = self.response.take() { let _ = response.send( result ); }
	}

	pub(crate) fn cancel( mut self ) {
		if let Some( response ) = self.response.take() {
			let _ = response.send( Err( runtime_error( AsyncRuntimeError::CallCancelled )));
		}
	}
}

impl<Ctx: 'static> Drop for AsyncRequest<Ctx> {
	fn drop( &mut self ) {
		if let Some( response ) = self.response.take() {
			let _ = response.send( Err( runtime_error( AsyncRuntimeError::CallCancelled )));
		}
	}
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
			.field( "state", &match &self.instance {
				PluginInstanceAny::Async( _ ) => "<session-managed async store>",
				PluginInstanceAny::Sync( _ ) => "<session-managed sync store>",
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
		} }
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
		linkage: AsyncLinkage<Ctx>,
		fuel_limiter: Option<CallLimiter<Ctx>>,
		epoch_limiter: Option<CallLimiter<Ctx>>,
	) -> Self {
		let AsyncLinkage { active_calls } = linkage;
		let mut state = PluginState {
			store,
			instance,
			interface_remaps,
			fuel_limiter,
			epoch_limiter,
		};
		Self {
			instance: PluginInstanceAny::Async({
				let ( sender, receiver ) = mpsc::unbounded();
				Arc::new( AsyncInstanceInner {
					sender,
					driver: StdMutex::new( DriverState::Idle( Box::pin( async move {
						state.run_requests( receiver, active_calls ).await
					}))),
				})
			}),
		}
	}

	pub(crate) fn handle( &self ) -> PluginDispatchHandle<Ctx> {
		PluginDispatchHandle { instance: self.instance.clone() }
	}

}

impl<Ctx: PluginContext + 'static> PluginDispatchHandle<Ctx> {
	pub(crate) fn dispatch_async(
		&self,
		dispatch: &DispatchContext<'_, Ctx>,
		package_name: &str,
		interface_name: &str,
		function_name: &str,
		function: &Function,
		data: &[Val],
	) -> BoxFuture<'static, Result<Val, DispatchError>> {
		if let Err( error ) = ensure_supported_values( data ) {
			return futures::future::ready( Err( error )).boxed();
		}
		let ( response, result ) = oneshot::channel();
		let path = dispatch.scheduler.child_path( dispatch.path, self.key() );
		let request = AsyncRequest {
			package_name: package_name.to_string(),
			interface_name: interface_name.to_string(),
			function_name: function_name.to_string(),
			function: function.clone(),
			data: data.to_vec(),
			response: Some( response ),
			active: ActiveCall {
				scheduler: dispatch.scheduler.clone(),
				caller: self.key(),
				path,
			},
		};
		dispatch.scheduler.schedule( dispatch.caller, path, self.clone(), request );
		receive_response( result ).boxed()
	}
	pub(crate) fn admit( &self, scheduler: &AsyncScheduler<Ctx>, request: AsyncRequest<Ctx> ) {
		match &self.instance {
			PluginInstanceAny::Async( inner ) => inner.enqueue( scheduler, request ),
			PluginInstanceAny::Sync( inner ) => {
				let inner = Arc::clone( inner );
				scheduler.attach_driver( Box::pin( async move {
					let mut state = inner.state.lock().await;
					let result = state.dispatch(
						&request.package_name,
						&request.interface_name,
						&request.function_name,
						&request.function,
						&request.data,
					);
					request.respond( result );
				}));
			}
		}
	}

	pub(crate) fn key( &self ) -> PluginKey {
		match &self.instance {
			PluginInstanceAny::Async( inner ) => PluginKey( Arc::as_ptr( inner ) as usize ),
			PluginInstanceAny::Sync( inner ) => PluginKey( Arc::as_ptr( inner ) as usize ),
		}
	}
}

impl<Ctx: PluginContext + 'static> From<PluginInstanceSync<Ctx>> for PluginInstanceAsync<Ctx> {
	/// Makes a synchronous instance usable in an async binding without changing
	/// how its Wasmtime store was instantiated.
	fn from( instance: PluginInstanceSync<Ctx> ) -> Self {
		let PluginInstanceSync { state } = instance;
		Self { instance: PluginInstanceAny::Sync( Arc::new( SyncInstanceInner {
			state: Mutex::new( state ),
		}))}
	}
}


impl<Ctx: PluginContext + 'static> AsyncInstanceInner<Ctx> {
	fn enqueue(
		self: &Arc<Self>,
		scheduler: &AsyncScheduler<Ctx>,
		request: AsyncRequest<Ctx>,
	) {
		let owner = scheduler.session_id();
		let mut driver = self.driver.lock().unwrap_or_else( std::sync::PoisonError::into_inner );
		let previous = std::mem::replace( &mut *driver, DriverState::Failed( AsyncRuntimeError::DriverStopped ));
		let future = match previous {
			DriverState::Idle( future ) => {
				*driver = DriverState::Attached { owner, waiting: VecDeque::new() };
				Some( future )
			},
			DriverState::Attached { owner: current, waiting } if current == owner => {
				*driver = DriverState::Attached { owner, waiting };
				None
			},
			DriverState::Attached { owner: current, mut waiting } => {
				if let Some( index ) = waiting.iter().position(| pending | pending.scheduler.session_id() == owner ) {
					waiting[index].requests.push_back( request );
					*driver = DriverState::Attached { owner: current, waiting };
					return;
				}
				waiting.push_back( PendingSession {
					scheduler: scheduler.clone(),
					requests: VecDeque::from([ request ]),
				});
				*driver = DriverState::Attached { owner: current, waiting };
				return;
			},
			DriverState::Failed( error ) => {
				request.respond( Err( runtime_error( error.clone() )));
				*driver = DriverState::Failed( error );
				return;
			},
		};
		if let Err( error ) = self.sender.unbounded_send( DriverMessage::Call( request )) {
			let DriverMessage::Call( request ) = error.into_inner();
			request.respond( Err( runtime_error( AsyncRuntimeError::DriverStopped )));
			*driver = DriverState::Failed( AsyncRuntimeError::DriverStopped );
			return;
		}
		drop( driver );
		if let Some( future ) = future {
			scheduler.attach_driver( Box::pin( AttachedDriver {
				inner: Arc::clone( self ),
				future: Some( future ),
			}));
		}
	}

}

impl<Ctx: PluginContext + 'static> AsyncInstanceInner<Ctx> {
	fn release( self: &Arc<Self>, future: DriverFuture ) {
		let ( scheduler, future, requests ) = {
			let mut driver = self.driver.lock().unwrap_or_else( std::sync::PoisonError::into_inner );
			let previous = std::mem::replace(
				&mut *driver,
				DriverState::Failed( AsyncRuntimeError::DriverStopped ),
			);
			match previous {
				DriverState::Attached { mut waiting, .. } => {
					let Some( pending ) = waiting.pop_front() else {
						*driver = DriverState::Idle( future );
						return;
					};
					let owner = pending.scheduler.session_id();
					*driver = DriverState::Attached { owner, waiting };
					( pending.scheduler, future, pending.requests )
				},
				DriverState::Failed( error ) => {
					*driver = DriverState::Failed( error );
					return;
				},
				DriverState::Idle( previous ) => {
					*driver = DriverState::Idle( previous );
					return;
				},
			}
		};
		for request in requests {
			if let Err( error ) = self.sender.unbounded_send( DriverMessage::Call( request )) {
				let DriverMessage::Call( request ) = error.into_inner();
				request.respond( Err( runtime_error( AsyncRuntimeError::DriverStopped )));
			}
		}
		scheduler.attach_driver( Box::pin( AttachedDriver { inner: Arc::clone( self ), future: Some( future ) }));
	}

	fn fail( &self, error: &AsyncRuntimeError ) {
		let waiting = match std::mem::replace(
			&mut *self.driver.lock().unwrap_or_else( std::sync::PoisonError::into_inner ),
			DriverState::Failed( error.clone() ),
		) {
			DriverState::Attached { waiting, .. } => waiting,
			_ => VecDeque::new(),
		};
		for pending in waiting {
			for request in pending.requests {
				request.respond( Err( runtime_error( error.clone() )));
			}
		}
	}
}

struct AttachedDriver<Ctx: PluginContext + 'static> {
	inner: Arc<AsyncInstanceInner<Ctx>>,
	future: Option<DriverFuture>,
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
			std::task::Poll::Ready( error ) => {
				self.future.take();
				self.inner.fail( &error );
				std::task::Poll::Ready(())
			}
		}
	}
}

impl<Ctx: PluginContext + 'static> Drop for AttachedDriver<Ctx> {
	fn drop( &mut self ) {
		if let Some( future ) = self.future.take() {
			self.inner.release( future );
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

	async fn run_requests(
		&mut self,
		mut receiver: mpsc::UnboundedReceiver<DriverMessage<Ctx>>,
		active_calls: ActiveCalls<Ctx>,
	) -> AsyncRuntimeError {
		if self.fuel_limiter.is_some() || self.epoch_limiter.is_some() {
			let mut queued = std::collections::VecDeque::new();
			loop {
				let message = match queued.pop_front() {
					Some( request ) => DriverMessage::Call( request ),
					None => match receiver.next().await {
						Some( message ) => message,
						None => return AsyncRuntimeError::RequestChannelClosed,
					},
				};
				let DriverMessage::Call( mut request ) = message;
				let mut response = request.response.take();
				let call = self.dispatch_async(
					&request.package_name,
					&request.interface_name,
					&request.function_name,
					&request.function,
					&request.data,
				).fuse();
				futures::pin_mut!( call );
				loop {
					futures::select_biased! {
						message = receiver.next().fuse() => match message {
							Some( DriverMessage::Call( pending )) => queued.push_back( pending ),
							None => {
								let result = call.await;
								if let Some( response ) = response.take() { let _ = response.send( result ); }
								break;
							},
						},
						result = call => {
							if let Some( response ) = response.take() { let _ = response.send( result ); }
							break;
						},
					}
				}
			}
		}

		let instance = self.instance;
		let interface_remaps = &self.interface_remaps;
		let run_result = self.store.run_concurrent( async | accessor | {
			let mut calls = FuturesUnordered::<BoxFuture<'_, ()>>::new();
			loop {
				if calls.is_empty() {
					let Some( message ) = receiver.next().await else { return };
					let DriverMessage::Call( request ) = message;
					start_concurrent_call( accessor, instance, interface_remaps, &active_calls, request, &mut calls );
					continue;
				}

				let request = receiver.next().fuse();
				let completed = calls.next().fuse();
				futures::pin_mut!( request, completed );
				futures::select_biased! {
					request = request => match request {
						Some( DriverMessage::Call( request )) => start_concurrent_call( accessor, instance, interface_remaps, &active_calls, request, &mut calls ),
						None => while calls.next().await.is_some() {},
					},
					_ = completed => {},
				}
			}
		}).await;

		if let Err( error ) = run_result {
			let error = AsyncRuntimeError::StoreFailed( error.to_string() );
			let active = std::mem::take(
				&mut *active_calls.lock().unwrap_or_else( std::sync::PoisonError::into_inner ),
			).into_values().collect();
			fail_active_calls( active, &error );
			while let Ok( message ) = receiver.try_recv() {
				let DriverMessage::Call( request ) = message;
				request.respond( Err( runtime_error( error.clone() )));
			}
			return error;
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

fn fail_active_calls<Ctx: 'static>( active: Vec<ActiveCallRecord<Ctx>>, error: &AsyncRuntimeError ) {
	for mut record in active {
		if let Some( response ) = record.response.take() {
			let _ = response.send( Err( runtime_error( error.clone() )));
		}
	}
}

fn start_concurrent_call<'a, Ctx: PluginContext + 'static>(
	accessor: &'a Accessor<Ctx>,
	instance: Instance,
	interface_remaps: &HashMap<String, Remap>,
	active_calls: &ActiveCalls<Ctx>,
	request: AsyncRequest<Ctx>,
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
			request.respond( Err( error ));
			return;
		}
	};
	let results = match request.function.return_kind() != ReturnKind::Void {
		true => vec![ PluginState::<Ctx>::PLACEHOLDER_VAL ],
		false => Vec::new(),
	};
	calls.push( concurrent_call( accessor, function, Arc::clone( active_calls ), request, results ));
}

fn concurrent_call<Ctx: PluginContext + 'static>(
	accessor: &Accessor<Ctx>,
	function: Func,
	active_calls: ActiveCalls<Ctx>,
	mut request: AsyncRequest<Ctx>,
	mut results: Vec<Val>,
) -> BoxFuture<'_, ()> {
	Box::pin( async move {
		let call = accessor.with(| mut access | function.start_call_concurrent(
			access.as_context_mut(),
			&request.data,
			&mut results,
		));
		let call = match call {
			Ok( call ) => call,
			Err( error ) => {
				request.respond( Err( DispatchError::RuntimeException( error )));
				return;
			}
		};
		let task = call.task();
		active_calls.lock().unwrap_or_else( std::sync::PoisonError::into_inner )
			.insert( task, ActiveCallRecord {
				active: request.active.clone(),
				response: request.response.take(),
			});
		let call_result = function.finish_call_concurrent( accessor, call ).await;
		let record = active_calls.lock().unwrap_or_else( std::sync::PoisonError::into_inner ).remove( &task );
		if let Some( response ) = record.and_then(| mut record | record.response.take() ) {
			let _ = response.send( PluginState::<Ctx>::finish_call( &request.function, results, call_result ));
		}
	})
}

fn runtime_error( error: AsyncRuntimeError ) -> DispatchError {
	DispatchError::RuntimeException( error.into() )
}

async fn receive_response(
	response: oneshot::Receiver<Result<Val, DispatchError>>,
) -> Result<Val, DispatchError> {
	match response.await {
		Ok( result ) => result,
		Err( _ ) => Err( runtime_error( AsyncRuntimeError::MissingResponse )),
	}
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
