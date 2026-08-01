use std::collections::{ BTreeMap, HashMap, VecDeque };
use std::future::Future;
use std::hash::Hash;
use std::pin::Pin;
use std::sync::{ Arc, Mutex as StdMutex };
use std::task::{ Context, Poll };

use futures::future::BoxFuture;
use futures::channel::oneshot;
use futures::task::AtomicWaker;
use thiserror::Error;
use wasmtime::component::{ GuestTaskId, Val };

use crate::plugin_instance::{ AsyncRequest, DispatchError, PluginDispatchHandle };
use crate::PluginContext;


#[derive( Clone, Copy, Debug, Eq, Hash, PartialEq )]
pub(crate) struct ExecutionPathId {
	serial: u64,
	depth: usize,
}

impl ExecutionPathId {
	pub(crate) const ROOT: Self = Self { serial: 0, depth: 0 };
}

#[derive( Clone, Copy, Debug, Eq, Hash, PartialEq )]
pub(crate) struct PluginKey( pub(crate) usize );

pub(crate) struct DispatchContext<'a, Ctx: 'static> {
	pub(crate) scheduler: &'a AsyncScheduler<Ctx>,
	pub(crate) caller: PluginKey,
	pub(crate) path: ExecutionPathId,
}

impl<Ctx: 'static> Copy for DispatchContext<'_, Ctx> {}

impl<Ctx: 'static> Clone for DispatchContext<'_, Ctx> {
	fn clone( &self ) -> Self { *self }
}

pub(crate) struct LinkDispatchContext<'a, Ctx: 'static> {
	pub(crate) calls: &'a ActiveCalls<Ctx>,
}

impl<'a, Ctx: 'static> LinkDispatchContext<'a, Ctx> {
	pub(crate) fn new( calls: &'a ActiveCalls<Ctx> ) -> Self {
		Self { calls }
	}
}

pub(crate) type ActiveCalls<Ctx> = Arc<StdMutex<HashMap<GuestTaskId, ActiveCallRecord<Ctx>>>>;

pub(crate) struct ActiveCallRecord<Ctx: 'static> {
	pub(crate) active: ActiveCall<Ctx>,
	pub(crate) response: Option<oneshot::Sender<Result<Val, DispatchError>>>,
}

pub(crate) struct ActiveCall<Ctx: 'static> {
	pub(crate) scheduler: AsyncScheduler<Ctx>,
	pub(crate) caller: PluginKey,
	pub(crate) path: ExecutionPathId,
}

impl<Ctx: 'static> Clone for ActiveCall<Ctx> {
	fn clone( &self ) -> Self {
		Self { scheduler: self.scheduler.clone(), caller: self.caller, path: self.path }
	}
}

impl<'a, Ctx: 'static> DispatchContext<'a, Ctx> {
	pub(crate) fn new( scheduler: &'a AsyncScheduler<Ctx>, caller: PluginKey, path: ExecutionPathId ) -> Self {
		Self { scheduler, caller, path }
	}
}


pub(crate) struct AsyncScheduler<Ctx: 'static> {
	shared: Arc<SchedulerShared<Ctx>>,
	origin: PluginKey,
}

impl<Ctx: 'static> Clone for AsyncScheduler<Ctx> {
	fn clone( &self ) -> Self {
		Self { shared: Arc::clone( &self.shared ), origin: self.origin }
	}
}

#[derive( Debug, Error )]
pub(crate) enum SchedulerUnavailable {
	#[error( "host import was not called by a guest task" )]
	MissingGuestTask,
	#[error( "guest task is not part of the active wasm-link dispatch" )]
	UnknownGuestTask,
}

pub(crate) fn active_call<Ctx: 'static>(
	calls: &ActiveCalls<Ctx>,
	task: GuestTaskId,
) -> Result<ActiveCall<Ctx>, SchedulerUnavailable> {
	calls.lock().unwrap_or_else( std::sync::PoisonError::into_inner )
		.get( &task ).map(| record | record.active.clone() ).ok_or( SchedulerUnavailable::UnknownGuestTask )
}

struct SchedulerShared<Ctx: 'static> {
	state: StdMutex<SchedulerState<Ctx>>,
	waker: AtomicWaker,
}

struct SchedulerState<Ctx: 'static> {
	closed: bool,
	next_path: u64,
	next_sequence: u64,
	paths: HashMap<( ExecutionPathId, PluginKey ), ExecutionPathId>,
	ready: BTreeMap<usize, HashMap<PluginKey, DestinationQueue<ScheduledCall<Ctx>>>>,
	drivers: Vec<BoxFuture<'static, ()>>,
}

impl<Ctx: PluginContext + 'static> AsyncScheduler<Ctx> {
	fn new() -> Self {
		Self { shared: Arc::new( SchedulerShared {
			state: StdMutex::new( SchedulerState {
				closed: false,
				next_path: 1,
				next_sequence: 0,
				paths: HashMap::new(),
				ready: BTreeMap::new(),
				drivers: Vec::new(),
			}),
			waker: AtomicWaker::new(),
		}), origin: PluginKey( 0 ) }
	}

	#[cfg(test)]
	pub(crate) fn testing() -> Self { Self::new() }

	pub(crate) fn origin( &self ) -> PluginKey { self.origin }

	pub(crate) fn session_id( &self ) -> usize { Arc::as_ptr( &self.shared ) as usize }

	pub(crate) fn child_path( &self, parent: ExecutionPathId, destination: PluginKey ) -> ExecutionPathId {
		let mut state = self.shared.state.lock().unwrap_or_else( std::sync::PoisonError::into_inner );
		if let Some( path ) = state.paths.get(&( parent, destination )) { return *path; }
		let path = ExecutionPathId { serial: state.next_path, depth: parent.depth + 1 };
		state.next_path += 1;
		state.paths.insert(( parent, destination ), path );
		path
	}

	pub(crate) fn schedule(
		&self,
		caller: PluginKey,
		path: ExecutionPathId,
		target: PluginDispatchHandle<Ctx>,
		request: AsyncRequest<Ctx>,
	) {
		let mut state = self.shared.state.lock().unwrap_or_else( std::sync::PoisonError::into_inner );
		if state.closed {
			request.cancel();
			return;
		}
		let destination = target.key();
		let call = ScheduledCall { sequence: state.next_sequence, target, request };
		state.next_sequence += 1;
		let queues = state.ready.entry( path.depth ).or_default();
		let queue = queues.remove( &destination ).unwrap_or_default();
		queues.insert( destination, queue.enqueue( caller, path, call ));
		drop( state );
		self.shared.waker.wake();
	}

	pub(crate) fn attach_driver( &self, driver: BoxFuture<'static, ()> ) {
		let mut state = self.shared.state.lock().unwrap_or_else( std::sync::PoisonError::into_inner );
		if state.closed { return; }
		state.drivers.push( driver );
		drop( state );
		self.shared.waker.wake();
	}

	fn take_call( &self ) -> Option<ScheduledCall<Ctx>> {
		let mut state = self.shared.state.lock().unwrap_or_else( std::sync::PoisonError::into_inner );
		let depth = state.ready.last_key_value().map(|( depth, _ )| *depth )?;
		let queues = state.ready.get_mut( &depth )?;
		let destination = oldest_destination( queues )?;
		let queue = queues.remove( &destination )?;
		let ( queue, call ) = queue.dequeue();
		if !queue.is_empty() { queues.insert( destination, queue ); }
		if queues.is_empty() { state.ready.remove( &depth ); }
		call
	}

	fn take_drivers( &self ) -> Vec<BoxFuture<'static, ()>> {
		let mut state = self.shared.state.lock().unwrap_or_else( std::sync::PoisonError::into_inner );
		std::mem::take( &mut state.drivers )
	}

	pub(crate) fn close( &self ) {
		let ready = {
			let mut state = self.shared.state.lock().unwrap_or_else( std::sync::PoisonError::into_inner );
			state.closed = true;
			std::mem::take( &mut state.ready )
		};
		drop( ready );
	}
}

fn oldest_destination<T: Created>( queues: &HashMap<PluginKey, DestinationQueue<T>> ) -> Option<PluginKey> {
	queues.iter()
		.min_by_key(|( _, queue )| queue.next_sequence())
		.map(|( destination, _ )| *destination)
}


struct ScheduledCall<Ctx: 'static> {
	sequence: u64,
	target: PluginDispatchHandle<Ctx>,
	request: AsyncRequest<Ctx>,
}

trait Created {
	fn sequence( &self ) -> u64;
}

impl<Ctx> Created for ScheduledCall<Ctx> {
	fn sequence( &self ) -> u64 { self.sequence }
}

impl<Ctx: PluginContext + 'static> ScheduledCall<Ctx> {
	fn admit( self, scheduler: &AsyncScheduler<Ctx> ) {
		self.target.admit( scheduler, self.request );
	}
}


struct DestinationQueue<T> {
	callers: RoundRobin<PluginKey, RoundRobin<ExecutionPathId, VecDeque<T>>>,
}

impl<T> Default for DestinationQueue<T> {
	fn default() -> Self { Self { callers: RoundRobin::default() } }
}

impl<T> DestinationQueue<T> {
	fn enqueue( mut self, caller: PluginKey, path: ExecutionPathId, call: T ) -> Self {
		self.callers = self.callers.update( caller, | paths | paths.update( path, | mut calls | {
			calls.push_back( call );
			calls
		}));
		self
	}

	fn dequeue( mut self ) -> ( Self, Option<T> ) {
		let Some(( caller, mut paths )) = self.callers.pop() else { return ( self, None )};
		let Some(( path, mut calls )) = paths.pop() else { return ( self, None )};
		let call = calls.pop_front();
		let paths = if calls.is_empty() { paths } else { paths.insert( path, calls ) };
		self.callers = if paths.is_empty() { self.callers } else { self.callers.insert( caller, paths ) };
		( self, call )
	}

	fn is_empty( &self ) -> bool { self.callers.is_empty() }
}

impl<T: Created> DestinationQueue<T> {
	fn next_sequence( &self ) -> u64 {
		self.callers.peek()
			.and_then(| paths | paths.peek() )
			.and_then( VecDeque::front )
			.map_or( u64::MAX, Created::sequence )
	}
}


struct RoundRobin<K, V> {
	order: VecDeque<K>,
	values: HashMap<K, V>,
}

impl<K, V> Default for RoundRobin<K, V> {
	fn default() -> Self { Self { order: VecDeque::new(), values: HashMap::new() } }
}

impl<K: Clone + Eq + Hash, V> RoundRobin<K, V> {
	fn insert( mut self, key: K, value: V ) -> Self {
		if !self.values.contains_key( &key ) { self.order.push_back( key.clone() ); }
		self.values.insert( key, value );
		self
	}

	fn update( mut self, key: K, update: impl FnOnce( V ) -> V ) -> Self
	where
		V: Default,
	{
		let value = if let Some( value ) = self.values.remove( &key ) {
			value
		} else {
			self.order.push_back( key.clone() );
			V::default()
		};
		self.values.insert( key, update( value ));
		self
	}

	fn pop( &mut self ) -> Option<( K, V )> {
		let key = self.order.pop_front()?;
		self.values.remove( &key ).map(| value | ( key, value ))
	}

	fn peek( &self ) -> Option<&V> {
		self.order.front().and_then(| key | self.values.get( key ))
	}

	fn is_empty( &self ) -> bool { self.order.is_empty() }
}


pub(crate) async fn run<Ctx, R, F, MakeFuture>( make_future: MakeFuture ) -> R
where
	Ctx: PluginContext + 'static,
	R: Send + 'static,
	F: Future<Output = R> + Send + 'static,
	MakeFuture: FnOnce( AsyncScheduler<Ctx> ) -> F,
{
	let scheduler = AsyncScheduler::new();
	let result = SchedulerFuture {
		root: Box::pin( make_future( scheduler.clone() )),
		scheduler,
		drivers: Vec::new(),
	}.await;
	result
}


struct SchedulerFuture<Ctx: PluginContext + 'static, F> {
	root: Pin<Box<F>>,
	scheduler: AsyncScheduler<Ctx>,
	drivers: Vec<BoxFuture<'static, ()>>,
}

impl<Ctx, F> Future for SchedulerFuture<Ctx, F>
where
	Ctx: PluginContext + 'static,
	F: Future,
{
	type Output = F::Output;

	fn poll( mut self: Pin<&mut Self>, cx: &mut Context<'_> ) -> Poll<Self::Output> {
		self.scheduler.shared.waker.register( cx.waker() );
		loop {
			if let Poll::Ready( result ) = self.root.as_mut().poll( cx ) {
				self.scheduler.close();
				return Poll::Ready( result );
			}

			if let Some( call ) = self.scheduler.take_call() {
				call.admit( &self.scheduler );
				let drivers = self.scheduler.take_drivers();
				self.drivers.extend( drivers );
				self.poll_drivers( cx );
				continue;
			}

			let drivers = self.scheduler.take_drivers();
			self.drivers.extend( drivers );
			if !self.poll_drivers( cx ) { return Poll::Pending; }
		}
	}
}

impl<Ctx: PluginContext + 'static, F> SchedulerFuture<Ctx, F> {
	fn poll_drivers( &mut self, cx: &mut Context<'_> ) -> bool {
		let mut completed = false;
		let mut index = 0;
		while index < self.drivers.len() {
			if Pin::new( &mut self.drivers[index] ).poll( cx ).is_ready() {
				drop( self.drivers.swap_remove( index ));
				completed = true;
			} else {
				index += 1;
			}
		}
		completed
	}
}

impl<Ctx: PluginContext + 'static, F> Drop for SchedulerFuture<Ctx, F> {
	fn drop( &mut self ) { self.scheduler.close(); }
}


#[cfg(test)]
mod tests {
	use std::collections::HashMap;

	use wasmtime::component::ResourceTable;

	use super::{ AsyncScheduler, Created, DestinationQueue, ExecutionPathId, PluginKey, oldest_destination };
	use crate::PluginContext;

	struct TestContext { resources: ResourceTable }

	impl PluginContext for TestContext {
		fn resource_table( &mut self ) -> &mut ResourceTable { &mut self.resources }
	}

	#[derive( Debug, Eq, PartialEq )]
	struct TestCall { sequence: u64, name: &'static str }

	impl Created for TestCall {
		fn sequence( &self ) -> u64 { self.sequence }
	}

	fn path( serial: u64 ) -> ExecutionPathId { ExecutionPathId { serial, depth: 0 } }

	#[test]
	fn destination_queue_rotates_callers_then_paths_and_preserves_path_fifo() {
		let caller_a = PluginKey( 1 );
		let caller_b = PluginKey( 2 );
		let mut queue = DestinationQueue::default()
			.enqueue( caller_a, path( 1 ), TestCall { sequence: 0, name: "a1-first" })
			.enqueue( caller_b, path( 3 ), TestCall { sequence: 3, name: "b1" })
			.enqueue( caller_a, path( 1 ), TestCall { sequence: 1, name: "a1-second" })
			.enqueue( caller_a, path( 2 ), TestCall { sequence: 2, name: "a2" });
		let mut order = Vec::new();
		while !queue.is_empty() {
			let ( remaining, call ) = queue.dequeue();
			queue = remaining;
			if let Some( call ) = call { order.push( call.name ); }
		}
		assert_eq!( order, [ "a1-first", "b1", "a2", "a1-second" ]);
	}

	#[test]
	fn scheduler_chooses_the_oldest_eligible_destination() {
		let caller = PluginKey( 1 );
		let older = PluginKey( 2 );
		let newer = PluginKey( 3 );
		let queues = HashMap::from([
			( newer, DestinationQueue::default().enqueue(
				caller, path( 1 ), TestCall { sequence: 2, name: "newer" },
			)),
			( older, DestinationQueue::default().enqueue(
				caller, path( 2 ), TestCall { sequence: 1, name: "older" },
			)),
		]);
		assert_eq!( oldest_destination( &queues ), Some( older ));
	}

	#[test]
	fn paths_are_reused_for_the_same_route_and_split_by_destination() {
		let mut context = TestContext { resources: ResourceTable::new() };
		let _ = context.resource_table();
		let scheduler = AsyncScheduler::<TestContext>::new();
		let root = ExecutionPathId::ROOT;
		let first = scheduler.child_path( root, PluginKey( 1 ));
		let same = scheduler.child_path( root, PluginKey( 1 ));
		let other = scheduler.child_path( root, PluginKey( 2 ));
		assert_eq!( first, same );
		assert_ne!( first, other );
		assert_eq!( first.depth, root.depth + 1 );
	}
}
