use std::collections::{ HashMap, HashSet, VecDeque };
use std::future::Future ;
use std::pin::Pin ;
use std::sync::{ Arc, Weak };
use futures::future::{ BoxFuture, Either };
use futures::stream::{ FuturesUnordered, Stream, StreamExt };
use futures::task::AtomicWaker ;
use thiserror::Error ;
use wasmtime::component::{ Accessor, Instance, Val };
use wasmtime::Store ;

use crate::{ Function, PluginContext, Remap, ReturnKind };
use crate::resource_wrapper::{ ResourceCreationError, ResourceReceiveError };

type CallLimiter<Ctx> = Box<dyn FnMut( &mut Store<Ctx>, &str, &str, &Function ) -> u64 + Send>;

pub(crate) struct DispatchSession {
	incoming: std::sync::Mutex<Vec<BoxFuture<'static, ()>>>,
	waker: AtomicWaker,
	external_caller: CallerToken,
}

#[derive( Clone )]
pub(crate) struct CallerToken( Arc<CallerMarker> );

struct CallerMarker;

impl PartialEq for CallerToken {
	fn eq( &self, other: &Self ) -> bool { Arc::ptr_eq( &self.0, &other.0 ) }
}

impl Eq for CallerToken {}

impl CallerToken {
	pub(crate) fn new() -> Self { Self( Arc::new( CallerMarker )) }
}

pub(crate) struct SessionSlot {
	current: std::sync::Mutex<Option<Weak<DispatchSession>>>,
}

impl SessionSlot {
	pub(crate) fn new() -> Arc<Self> {
		Arc::new( Self { current: std::sync::Mutex::new( None )})
	}

	pub(crate) fn current( &self ) -> Option<Arc<DispatchSession>> {
		lock_unpoisoned( &self.current ).as_ref().and_then( Weak::upgrade )
	}

	fn set( &self, session: Option<&Arc<DispatchSession>> ) {
		*lock_unpoisoned( &self.current ) = session.map( Arc::downgrade );
	}
}

#[derive( Clone )]
pub(crate) struct AsyncLinkContext {
	pub(crate) session_slot: Arc<SessionSlot>,
	pub(crate) caller: CallerToken,
}

impl AsyncLinkContext {
	pub(crate) fn new() -> Self {
		Self {
			session_slot: SessionSlot::new(),
			caller: CallerToken::new(),
		}
	}
}

impl DispatchSession {
	pub(crate) fn new() -> Arc<Self> {
		Arc::new( Self {
			incoming: std::sync::Mutex::new( Vec::new() ),
			waker: AtomicWaker::new(),
			external_caller: CallerToken::new(),
		})
	}

	pub(crate) fn external_caller( &self ) -> &CallerToken { &self.external_caller }

	pub(crate) fn spawn( &self, future: BoxFuture<'static, ()> ) {
		lock_unpoisoned( &self.incoming ).push( future );
		self.waker.wake();
	}

	pub(crate) async fn run<F>( self: &Arc<Self>, future: F ) -> F::Output
	where
		F: Future,
	{
		let mut future = std::pin::pin!( future );
		let mut tasks = FuturesUnordered::<BoxFuture<'static, ()>>::new();
		futures::future::poll_fn(| cx | {
			self.waker.register( cx.waker() );
			if let std::task::Poll::Ready( output ) = future.as_mut().poll( cx ) {
				return std::task::Poll::Ready( output );
			}

			loop {
				tasks.extend( lock_unpoisoned( &self.incoming ).drain( .. ));
				if !matches!(
					Pin::new( &mut tasks ).poll_next( cx ),
					std::task::Poll::Ready( Some(()))
				) && lock_unpoisoned( &self.incoming ).is_empty() {
					return std::task::Poll::Pending;
				}
			}
		}).await
	}
}

pub(crate) trait AsyncDispatchInstance<Ctx>:
	ExportEffectInstance + Clone + Send + Sync + 'static
where
	Ctx: PluginContext + 'static,
{
	#[allow( clippy::too_many_arguments )]
	fn dispatch_for_async<'a>(
		&'a self,
		session: &'a Arc<DispatchSession>,
		caller: &'a CallerToken,
		package_name: &'a str,
		interface_name: &'a str,
		function_name: &'a str,
		function: &'a Function,
		data: &'a [Val],
	) -> BoxFuture<'a, Result<Val, DispatchError>>;
}

pub(crate) trait ExportEffectInstance {
	fn export_is_async(
		&self,
		package_name: &str,
		interface_name: &str,
		function_name: &str,
	) -> bool;
}


/// A synchronously instantiated plugin, ready for synchronous dispatch.
///
/// Created by calling [`Plugin::instantiate`]( crate::Plugin::instantiate ),
/// or [`Plugin::link`]( crate::Plugin::link ).
pub struct PluginInstanceSync<Ctx: 'static> {
	state: Arc<std::sync::Mutex<PluginState<Ctx>>>,
	metadata: Arc<PluginMetadata>,
}

impl<Ctx: 'static> Clone for PluginInstanceSync<Ctx> {
	fn clone( &self ) -> Self {
		Self {
			state: Arc::clone( &self.state ),
			metadata: Arc::clone( &self.metadata ),
		}
	}
}

/// An asynchronously instantiated plugin, ready for asynchronous dispatch.
///
/// Created by calling [`Plugin::instantiate_async`]( crate::Plugin::instantiate_async )
/// or [`Plugin::link_async`]( crate::Plugin::link_async ). Calls from one dispatch
/// session are cooperatively scheduled on the plugin's Wasmtime [`Store`].
pub struct PluginInstanceAsync<Ctx: 'static> {
	dispatcher: Arc<InstanceDispatcher<Ctx>>,
	metadata: Arc<PluginMetadata>,
}

impl<Ctx: 'static> Clone for PluginInstanceAsync<Ctx> {
	fn clone( &self ) -> Self {
		Self {
			dispatcher: Arc::clone( &self.dispatcher ),
			metadata: Arc::clone( &self.metadata ),
		}
	}
}

struct InstanceDispatcher<Ctx: 'static> {
	state: std::sync::Mutex<Option<PluginState<Ctx>>>,
	queue: std::sync::Mutex<DispatchQueue>,
	waker: AtomicWaker,
	session_slot: Arc<SessionSlot>,
}

struct DispatchQueue {
	active: Option<SessionBatch>,
	waiting: VecDeque<SessionBatch>,
	running: bool,
	closing: bool,
}

struct SessionBatch {
	session: Weak<DispatchSession>,
	callers: VecDeque<CallerQueue>,
}

struct CallerQueue {
	caller: CallerToken,
	requests: VecDeque<CallRequest>,
}

struct CallRequest {
	package_name: String,
	interface_name: String,
	function_name: String,
	function: Function,
	data: Vec<Val>,
	response: Arc<CallResponse>,
}

struct CallResponse {
	sender: std::sync::Mutex<Option<futures::channel::oneshot::Sender<Result<Val, DispatchError>>>>,
}

impl CallResponse {
	fn new( sender: futures::channel::oneshot::Sender<Result<Val, DispatchError>> ) -> Arc<Self> {
		Arc::new( Self { sender: std::sync::Mutex::new( Some( sender ))})
	}

	fn send( &self, result: Result<Val, DispatchError> ) {
		if let Some( sender ) = lock_unpoisoned( &self.sender ).take() {
			let _ = sender.send( result );
		}
	}
}

struct PluginState<Ctx: 'static> {
	store: Store<Ctx>,
	instance: Instance,
	metadata: Arc<PluginMetadata>,
	fuel_limiter: Option<CallLimiter<Ctx>>,
	epoch_limiter: Option<CallLimiter<Ctx>>,
}

struct PluginMetadata {
	interface_remaps: HashMap<String, Remap>,
	async_exports: HashSet<(String, String)>,
}

impl<Ctx: std::fmt::Debug + 'static> std::fmt::Debug for PluginInstanceSync<Ctx> {
	fn fmt( &self, f: &mut std::fmt::Formatter<'_> ) -> std::result::Result<(), std::fmt::Error> {
		let state = lock_unpoisoned( &self.state );
		f.debug_struct( "PluginInstanceSync" )
			.field( "data", &state.store.data() )
			.field( "store", &state.store )
			.field( "interface_remaps", &state.metadata.interface_remaps )
			.field( "fuel_limiter", &state.fuel_limiter.as_ref().map(| _ | "<closure>" ))
			.field( "epoch_limiter", &state.epoch_limiter.as_ref().map(| _ | "<closure>" ))
			.finish_non_exhaustive()
	}
}

impl<Ctx: 'static> std::fmt::Debug for PluginInstanceAsync<Ctx> {
	fn fmt( &self, f: &mut std::fmt::Formatter<'_> ) -> std::result::Result<(), std::fmt::Error> {
		f.debug_struct( "PluginInstanceAsync" )
			.field( "state", &"<cooperatively dispatched store>" )
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
		async_exports: HashSet<(String, String)>,
	) -> Self {
		let metadata = Arc::new( PluginMetadata { interface_remaps, async_exports });
		Self {
			state: Arc::new( std::sync::Mutex::new( PluginState {
				store,
				instance,
				metadata: Arc::clone( &metadata ),
				fuel_limiter,
				epoch_limiter,
			})),
			metadata,
		}
	}

	pub(crate) fn dispatch_from(
		&self,
		package_name: &str,
		interface_name: &str,
		function_name: &str,
		function: &Function,
		data: &[Val],
	) -> Result<Val, DispatchError> {
		lock_unpoisoned( &self.state )
			.dispatch( package_name, interface_name, function_name, function, data )
	}
}

impl<Ctx: 'static> ExportEffectInstance for PluginInstanceSync<Ctx> {
	fn export_is_async(
		&self,
		package_name: &str,
		interface_name: &str,
		function_name: &str,
	) -> bool {
		let export = resolve_export(
			&self.metadata.interface_remaps,
			package_name,
			interface_name,
			function_name,
		);
		self.metadata.async_exports.contains( &export )
	}
}

impl<Ctx> AsyncDispatchInstance<Ctx> for PluginInstanceSync<Ctx>
where
	Ctx: PluginContext + 'static,
{
	fn dispatch_for_async<'a>(
		&'a self,
		session: &'a Arc<DispatchSession>,
		_caller: &'a CallerToken,
		package_name: &'a str,
		interface_name: &'a str,
		function_name: &'a str,
		function: &'a Function,
		data: &'a [Val],
	) -> BoxFuture<'a, Result<Val, DispatchError>> {
		let instance = self.clone();
		let session = Arc::clone( session );
		let package_name = package_name.to_string();
		let interface_name = interface_name.to_string();
		let function_name = function_name.to_string();
		let function = function.clone();
		let data = data.to_vec();
		Box::pin( async move {
			let ( response, result ) = futures::channel::oneshot::channel();
			session.spawn( Box::pin( async move {
				let result = instance.dispatch_from(
					&package_name, &interface_name, &function_name, &function, &data,
				);
				let _ = response.send( result );
			}));
			result.await.map_err(| _ | DispatchError::MissingResponse )?
		})
	}
}

impl<Ctx> PluginInstanceAsync<Ctx>
where
	Ctx: PluginContext + 'static,
{
	pub(crate) fn new(
		store: Store<Ctx>,
		instance: Instance,
		interface_remaps: HashMap<String, Remap>,
		fuel_limiter: Option<CallLimiter<Ctx>>,
		epoch_limiter: Option<CallLimiter<Ctx>>,
		async_exports: HashSet<(String, String)>,
		link_context: AsyncLinkContext,
	) -> Self {
		let metadata = Arc::new( PluginMetadata { interface_remaps, async_exports });
		Self {
			dispatcher: Arc::new( InstanceDispatcher {
				state: std::sync::Mutex::new( Some( PluginState {
					store,
					instance,
					metadata: Arc::clone( &metadata ),
					fuel_limiter,
					epoch_limiter,
				})),
				queue: std::sync::Mutex::new( DispatchQueue {
					active: None,
					waiting: VecDeque::new(),
					running: false,
					closing: false,
				}),
				waker: AtomicWaker::new(),
				session_slot: link_context.session_slot,
			}),
			metadata,
		}
	}

	#[allow( clippy::too_many_arguments )]
	pub(crate) async fn dispatch_async_from(
		&self,
		session: &Arc<DispatchSession>,
		caller: &CallerToken,
		package_name: &str,
		interface_name: &str,
		function_name: &str,
		function: &Function,
		data: &[Val],
	) -> Result<Val, DispatchError> {
		ensure_supported_values( data )?;
		let ( response, result ) = futures::channel::oneshot::channel();
		self.dispatcher.submit(
			session,
			caller.clone(),
			CallRequest {
				package_name: package_name.to_string(),
				interface_name: interface_name.to_string(),
				function_name: function_name.to_string(),
				function: function.clone(),
				data: data.to_vec(),
				response: CallResponse::new( response ),
			},
		);
		result.await.map_err(| _ | DispatchError::MissingResponse )?
	}
}

impl<Ctx: 'static> ExportEffectInstance for PluginInstanceAsync<Ctx> {
	fn export_is_async(
		&self,
		package_name: &str,
		interface_name: &str,
		function_name: &str,
	) -> bool {
		let export = resolve_export(
			&self.metadata.interface_remaps,
			package_name,
			interface_name,
			function_name,
		);
		self.metadata.async_exports.contains( &export )
	}
}

impl<Ctx> AsyncDispatchInstance<Ctx> for PluginInstanceAsync<Ctx>
where
	Ctx: PluginContext + 'static,
{
	fn dispatch_for_async<'a>(
		&'a self,
		session: &'a Arc<DispatchSession>,
		caller: &'a CallerToken,
		package_name: &'a str,
		interface_name: &'a str,
		function_name: &'a str,
		function: &'a Function,
		data: &'a [Val],
	) -> BoxFuture<'a, Result<Val, DispatchError>> {
		Box::pin( self.dispatch_async_from(
			session,
			caller,
			package_name,
			interface_name,
			function_name,
			function,
			data,
		))
	}
}

impl SessionBatch {
	fn new( session: &Arc<DispatchSession> ) -> Self {
		Self { session: Arc::downgrade( session ), callers: VecDeque::new() }
	}

	fn belongs_to( &self, session: &Arc<DispatchSession> ) -> bool {
		self.session.ptr_eq( &Arc::downgrade( session ))
	}

	fn push( &mut self, caller: CallerToken, request: CallRequest ) {
		match self.callers.iter_mut().find(| queue | queue.caller == caller ) {
			Some( queue ) => queue.requests.push_back( request ),
			None => self.callers.push_back( CallerQueue {
				caller,
				requests: VecDeque::from([ request ]),
			}),
		}
	}

	fn pop( &mut self ) -> Option<CallRequest> {
		let mut caller = self.callers.pop_front()?;
		let request = caller.requests.pop_front();
		if !caller.requests.is_empty() {
			self.callers.push_back( caller );
		}
		request
	}
}

impl<Ctx> InstanceDispatcher<Ctx>
where
	Ctx: PluginContext + 'static,
{
	fn submit(
		self: &Arc<Self>,
		session: &Arc<DispatchSession>,
		caller: CallerToken,
		request: CallRequest,
	) {
		let start = {
			let mut queue = lock_unpoisoned( &self.queue );
			Self::clear_cancelled_active( &mut queue );
			if queue.closing {
				Self::push_waiting( &mut queue.waiting, session, caller, request );
				None
			} else if let Some( active ) = queue.active.as_mut()
				.filter(| active | active.belongs_to( session ))
			{
				active.push( caller, request );
				None
			} else {
				Self::push_waiting( &mut queue.waiting, session, caller, request );
				Self::start_if_idle( &mut queue )
			}
		};
		self.waker.wake();
		if let Some( session ) = start {
			self.spawn_batch( &session );
		}
	}

	fn push_waiting(
		waiting: &mut VecDeque<SessionBatch>,
		session: &Arc<DispatchSession>,
		caller: CallerToken,
		request: CallRequest,
	) {
		if let Some( batch ) = waiting.iter_mut()
			.find(| batch | batch.belongs_to( session ))
		{
			batch.push( caller, request );
		} else {
			let mut batch = SessionBatch::new( session );
			batch.push( caller, request );
			waiting.push_back( batch );
		}
	}

	fn clear_cancelled_active( queue: &mut DispatchQueue ) {
		if queue.running && queue.active.as_ref().is_some_and(| active |
			active.session.upgrade().is_none()
		) {
			queue.active = None;
			queue.running = false;
			queue.closing = false;
		}
	}

	fn start_if_idle( queue: &mut DispatchQueue ) -> Option<Arc<DispatchSession>> {
		if queue.running {
			return None;
		}
		while let Some( batch ) = queue.waiting.pop_front() {
			if let Some( session ) = batch.session.upgrade() {
				queue.active = Some( batch );
				queue.running = true;
				queue.closing = false;
				return Some( session );
			}
		}
		None
	}

	fn spawn_batch( self: &Arc<Self>, session: &Arc<DispatchSession> ) {
		let dispatcher = Arc::clone( self );
		let batch_session = Arc::downgrade( session );
		session.spawn( Box::pin( async move {
			if let Some( session ) = batch_session.upgrade() {
				dispatcher.run_batch( session ).await;
			}
		}));
	}

	fn pop_request( &self, session: &Arc<DispatchSession> ) -> Option<CallRequest> {
		let mut queue = lock_unpoisoned( &self.queue );
		if queue.closing {
			return None;
		}
		queue.active.as_mut()
			.filter(| active | active.belongs_to( session ))
			.and_then( SessionBatch::pop )
	}

	fn begin_closing( &self, session: &Arc<DispatchSession> ) -> bool {
		let mut queue = lock_unpoisoned( &self.queue );
		let empty = queue.active.as_ref()
			.filter(| active | active.belongs_to( session ))
			.is_some_and(| active | active.callers.is_empty() );
		if empty {
			queue.closing = true;
		}
		empty
	}

	fn poll_request_ready(
		&self,
		session: &Arc<DispatchSession>,
		cx: &mut std::task::Context<'_>,
	) -> std::task::Poll<()> {
		self.waker.register( cx.waker() );
		let queue = lock_unpoisoned( &self.queue );
		let ready = queue.closing || queue.active.as_ref()
			.filter(| active | active.belongs_to( session ))
			.is_none_or(| active | !active.callers.is_empty() );
		match ready {
			true => std::task::Poll::Ready(()),
			false => std::task::Poll::Pending,
		}
	}

	async fn run_batch( self: Arc<Self>, session: Arc<DispatchSession> ) {
		let state = lock_unpoisoned( &self.state ).take();
		let Some( state ) = state else {
			self.finish_batch();
			return;
		};
		let mut lease = PluginStateLease {
			dispatcher: Arc::clone( &self ),
			state: Some( state ),
		};
		self.session_slot.set( Some( &session ));

		let Some( state ) = lease.state.as_mut() else { return };
		if state.fuel_limiter.is_some() || state.epoch_limiter.is_some() {
			self.run_serial_batch( state, &session ).await;
		} else {
			self.run_concurrent_batch( state, &session ).await;
		}
	}

	async fn run_serial_batch(
		&self,
		state: &mut PluginState<Ctx>,
		session: &Arc<DispatchSession>,
	) {
		loop {
			if let Some( request ) = self.pop_request( session ) {
				let result = state.dispatch_async(
					&request.package_name,
					&request.interface_name,
					&request.function_name,
					&request.function,
					&request.data,
				).await;
				request.response.send( result );
				continue;
			}
			let _ = self.begin_closing( session );
			break;
		}
	}

	async fn run_concurrent_batch(
		&self,
		state: &mut PluginState<Ctx>,
		session: &Arc<DispatchSession>,
	) {
		let instance = state.instance;
		let metadata = Arc::clone( &state.metadata );
		let mut admitted = Vec::new();
		let result = state.store.run_concurrent( async | accessor | {
			let mut calls = FuturesUnordered::<BoxFuture<'_, ()>>::new();
			loop {
				while let Some( request ) = self.pop_request( session ) {
					admitted.push( Arc::clone( &request.response ));
					calls.push( concurrent_call(
						accessor,
						instance,
						Arc::clone( &metadata ),
						request,
					));
				}
				if calls.is_empty() && self.begin_closing( session ) {
					break;
				}

				let request_ready = futures::future::poll_fn(| cx |
					self.poll_request_ready( session, cx )
				);
				futures::pin_mut!( request_ready );
				let call_ready = calls.next();
				futures::pin_mut!( call_ready );
				match futures::future::select( call_ready, request_ready ).await {
					Either::Left(( _, _ )) | Either::Right(((), _ )) => {}
				}
			}
		}).await;
		self.finish_concurrent_batch( session, admitted, result );
	}

	fn finish_concurrent_batch(
		&self,
		session: &Arc<DispatchSession>,
		mut admitted: Vec<Arc<CallResponse>>,
		result: wasmtime::Result<()>,
	) {
		let Err( error ) = result else { return };
		let queued = {
			let mut queue = lock_unpoisoned( &self.queue );
			queue.active.as_mut()
				.filter(| active | active.belongs_to( session ))
				.map(| active | active.callers.drain( .. )
					.flat_map(| caller | caller.requests )
					.map(| request | request.response )
					.collect::<Vec<_>>()
				)
				.unwrap_or_default()
		};
		admitted.extend( queued );
		let message = error.to_string();
		for response in admitted {
			response.send( Err( DispatchError::RuntimeException(
				wasmtime::Error::msg( message.clone() ),
			)));
		}
	}

	fn finish_batch( self: &Arc<Self> ) {
		self.session_slot.set( None );
		let start = {
			let mut queue = lock_unpoisoned( &self.queue );
			queue.active = None;
			queue.running = false;
			queue.closing = false;
			Self::start_if_idle( &mut queue )
		};
		if let Some( session ) = start {
			self.spawn_batch( &session );
		}
	}
}

struct PluginStateLease<Ctx: PluginContext + 'static> {
	dispatcher: Arc<InstanceDispatcher<Ctx>>,
	state: Option<PluginState<Ctx>>,
}

impl<Ctx: PluginContext + 'static> Drop for PluginStateLease<Ctx> {
	fn drop( &mut self ) {
		if let Some( state ) = self.state.take() {
			*lock_unpoisoned( &self.dispatcher.state ) = Some( state );
		}
		self.dispatcher.finish_batch();
	}
}

fn concurrent_call<Ctx>(
	accessor: &Accessor<Ctx>,
	instance: Instance,
	metadata: Arc<PluginMetadata>,
	request: CallRequest,
) -> BoxFuture<'_, ()>
where
	Ctx: PluginContext + 'static,
{
	Box::pin( async move {
		let mut buffer = PluginState::<Ctx>::result_buffer( &request.function );
		let ( interface_path, function_name ) = resolve_export(
			&metadata.interface_remaps,
			&request.package_name,
			&request.interface_name,
			&request.function_name,
		);
		let function = accessor.with(| mut access |
			function_from( instance, &mut access, &interface_path, &function_name )
		);
		let result = match function {
			Ok( function ) => {
				let call_result = function.call_concurrent(
					accessor,
					&request.data,
					&mut buffer,
				).await;
				PluginState::<Ctx>::finish_call( &request.function, buffer, call_result )
			}
			Err( error ) => Err( error ),
		};
		request.response.send( result );
	})
}

fn lock_unpoisoned<T>( mutex: &std::sync::Mutex<T> ) -> std::sync::MutexGuard<'_, T> {
	mutex.lock().unwrap_or_else( std::sync::PoisonError::into_inner )
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
		let func = function_from(
			self.instance,
			&mut self.store,
			&exported_interface_path,
			&exported_function_name,
		)?;
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
		let func = function_from(
			self.instance,
			&mut self.store,
			&exported_interface_path,
			&exported_function_name,
		)?;
		let call_result = func.call_async( &mut self.store, data, &mut buffer ).await;
		Self::finish_call( function, buffer, call_result )
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
		Ok( Self::result_buffer( function ))
	}

	fn result_buffer( function: &Function ) -> Vec<Val> {
		match function.return_kind() != ReturnKind::Void {
			true => vec![ Self::PLACEHOLDER_VAL ],
			false => Vec::with_capacity( 0 ),
		}
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
		resolve_export( &self.metadata.interface_remaps, package_name, interface_name, function_name )
	}

}

fn function_from(
	instance: Instance,
	mut store: impl wasmtime::AsContextMut,
	interface_path: &str,
	function_name: &str,
) -> Result<wasmtime::component::Func, DispatchError> {
	let mut store = store.as_context_mut();
	let interface_index = instance
		.get_export_index( &mut store, None, interface_path )
		.ok_or_else(|| DispatchError::InvalidInterfacePath( interface_path.to_string() ))?;
	let func_index = instance
		.get_export_index( &mut store, Some( &interface_index ), function_name )
		.ok_or_else(|| DispatchError::InvalidFunction( format!( "{interface_path}:{function_name}" )))?;
	instance
		.get_func( &mut store, func_index )
		.ok_or_else(|| DispatchError::InvalidFunction( format!( "{interface_path}:{function_name}" )))
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
		None => (
			format!( "{}/{}", package_name, interface_name ),
			function_name.to_string(),
		),
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
