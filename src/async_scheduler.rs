use std::collections::{ BTreeMap, HashMap, VecDeque };
use std::future::Future;
use std::hash::Hash;
use std::pin::Pin;
use std::sync::{ Arc, Mutex as StdMutex };
use std::task::{ Context, Poll };

use futures::future::BoxFuture;
use futures::lock::Mutex;
use futures::task::AtomicWaker;
use thiserror::Error;

use crate::plugin_instance::{ AsyncRequest, PluginInstanceAsync };
use crate::PluginContext;


#[derive( Clone, Debug )]
pub(crate) struct PluginNode( Arc<PluginNodeData> );

#[derive( Debug )]
struct PluginNodeData {
	gate: Mutex<()>,
	dependencies: Vec<PluginNode>,
}

impl PluginNode {
	pub(crate) fn new( dependencies: Vec<Self> ) -> Self {
		Self( Arc::new( PluginNodeData {
			gate: Mutex::new(()),
			dependencies,
		}))
	}

	fn address( &self ) -> usize { Arc::as_ptr( &self.0 ) as usize }

	pub(crate) fn key( &self ) -> PluginKey { PluginKey( self.address() ) }
}

#[derive( Clone, Copy, Debug, Eq, Hash, PartialEq )]
pub(crate) struct ExecutionPathId {
	serial: u64,
	depth: usize,
}

#[derive( Clone, Copy, Debug, Eq, Hash, PartialEq )]
pub(crate) struct PluginKey( usize );

pub(crate) struct DispatchContext<'a, Ctx: 'static> {
	pub(crate) scheduler: &'a AsyncScheduler<Ctx>,
	pub(crate) caller: PluginKey,
	pub(crate) path: ExecutionPathId,
}

pub(crate) struct LinkDispatchContext<'a, Ctx: 'static> {
	pub(crate) caller: PluginKey,
	pub(crate) scheduler_slot: &'a Arc<SchedulerSlot<Ctx>>,
}

impl<'a, Ctx: 'static> LinkDispatchContext<'a, Ctx> {
	pub(crate) fn new( caller: PluginKey, scheduler_slot: &'a Arc<SchedulerSlot<Ctx>> ) -> Self {
		Self { caller, scheduler_slot }
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

pub(crate) struct SchedulerSlot<Ctx: 'static> {
	scheduler: StdMutex<Option<AsyncScheduler<Ctx>>>,
}

#[derive( Debug, Error )]
#[error( "plugin call has no active async dispatch scheduler" )]
pub(crate) struct SchedulerUnavailable;

impl<Ctx: PluginContext + 'static> SchedulerSlot<Ctx> {
	pub(crate) fn new() -> Arc<Self> {
		Arc::new( Self { scheduler: StdMutex::new( None ) })
	}

	pub(crate) fn require( &self ) -> Result<AsyncScheduler<Ctx>, SchedulerUnavailable> {
		self.scheduler.lock().unwrap_or_else( std::sync::PoisonError::into_inner ).clone().ok_or( SchedulerUnavailable )
	}

	pub(crate) fn attach( self: &Arc<Self>, scheduler: &AsyncScheduler<Ctx> ) -> SchedulerSlotGuard<Ctx> {
		let previous = self.scheduler.lock().unwrap_or_else( std::sync::PoisonError::into_inner ).replace( scheduler.clone() );
		SchedulerSlotGuard { slot: Arc::clone( self ), previous }
	}
}

pub(crate) struct SchedulerSlotGuard<Ctx: 'static> {
	slot: Arc<SchedulerSlot<Ctx>>,
	previous: Option<AsyncScheduler<Ctx>>,
}

impl<Ctx: 'static> Drop for SchedulerSlotGuard<Ctx> {
	fn drop( &mut self ) {
		*self.slot.scheduler.lock().unwrap_or_else( std::sync::PoisonError::into_inner ) = self.previous.take();
	}
}

struct SchedulerShared<Ctx: 'static> {
	state: StdMutex<SchedulerState<Ctx>>,
	waker: AtomicWaker,
}

struct SchedulerState<Ctx: 'static> {
	closed: bool,
	next_path: u64,
	next_sequence: u64,
	depths: HashMap<PluginKey, usize>,
	ready: BTreeMap<usize, HashMap<PluginKey, DestinationQueue<ScheduledCall<Ctx>>>>,
	drivers: Vec<BoxFuture<'static, ()>>,
}

impl<Ctx: PluginContext + 'static> AsyncScheduler<Ctx> {
	fn new( depths: HashMap<PluginKey, usize> ) -> Self {
		Self { shared: Arc::new( SchedulerShared {
			state: StdMutex::new( SchedulerState {
				closed: false,
				next_path: 1,
				next_sequence: 0,
				depths,
				ready: BTreeMap::new(),
				drivers: Vec::new(),
			}),
			waker: AtomicWaker::new(),
		}), origin: PluginKey( 0 ) }
	}

	#[cfg(test)]
	pub(crate) fn testing() -> Self { Self::new( HashMap::new() ) }

	pub(crate) fn origin( &self ) -> PluginKey { self.origin }

	pub(crate) fn root_path( &self ) -> ExecutionPathId {
		self.new_path( 0 )
	}

	pub(crate) fn execution_path( &self, caller: PluginKey ) -> ExecutionPathId {
		let mut state = self.shared.state.lock().unwrap_or_else( std::sync::PoisonError::into_inner );
		let depth = state.depths.get( &caller ).copied().unwrap_or_default() + 1;
		let path = ExecutionPathId { serial: state.next_path, depth };
		state.next_path += 1;
		path
	}

	fn new_path( &self, depth: usize ) -> ExecutionPathId {
		let mut state = self.shared.state.lock().unwrap_or_else( std::sync::PoisonError::into_inner );
		let path = ExecutionPathId { serial: state.next_path, depth };
		state.next_path += 1;
		path
	}

	pub(crate) fn schedule(
		&self,
		caller: PluginKey,
		path: ExecutionPathId,
		target: PluginInstanceAsync<Ctx>,
		request: AsyncRequest,
	) {
		let mut state = self.shared.state.lock().unwrap_or_else( std::sync::PoisonError::into_inner );
		if state.closed {
			request.cancel();
			return;
		}
		let destination = target.plugin_node().key();
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
	target: PluginInstanceAsync<Ctx>,
	request: AsyncRequest,
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


pub(crate) async fn run<Ctx, R, F, MakeFuture>( roots: Vec<PluginNode>, make_future: MakeFuture ) -> R
where
	Ctx: PluginContext + 'static,
	R: Send + 'static,
	F: Future<Output = R> + Send + 'static,
	MakeFuture: FnOnce( AsyncScheduler<Ctx> ) -> F,
{
	let ( nodes, depths ) = graph_metadata( &roots );
	let mut guards = Vec::with_capacity( nodes.len() );
	for node in &nodes { guards.push( node.0.gate.lock().await ); }
	let scheduler = AsyncScheduler::new( depths );
	let result = SchedulerFuture {
		root: Box::pin( make_future( scheduler.clone() )),
		scheduler,
		drivers: Vec::new(),
	}.await;
	drop( guards );
	result
}

fn graph_metadata( roots: &[PluginNode] ) -> ( Vec<PluginNode>, HashMap<PluginKey, usize> ) {
	fn visit( node: &PluginNode, depth: usize, depths: &mut HashMap<PluginKey, ( PluginNode, usize )> ) {
		let key = node.key();
		if depths.get( &key ).is_some_and(|( _, previous )| *previous >= depth ) { return; }
		depths.insert( key, ( node.clone(), depth ));
		node.0.dependencies.iter().for_each(| dependency | visit( dependency, depth + 1, depths ));
	}

	let mut depths = HashMap::new();
	for root in roots { visit( root, 0, &mut depths ); }
	let mut nodes = depths.values().map(|( node, _ )| node.clone()).collect::<Vec<_>>();
	nodes.sort_unstable_by_key( PluginNode::address );
	( nodes, depths.into_iter().map(|( key, ( _, depth ))| ( key, depth )).collect())
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

	use super::{ AsyncScheduler, Created, DestinationQueue, ExecutionPathId, PluginNode, oldest_destination };
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
		let caller_a = PluginNode::new( Vec::new() );
		let caller_b = PluginNode::new( Vec::new() );
		let mut queue = DestinationQueue::default()
			.enqueue( caller_a.key(), path( 1 ), TestCall { sequence: 0, name: "a1-first" })
			.enqueue( caller_b.key(), path( 3 ), TestCall { sequence: 3, name: "b1" })
			.enqueue( caller_a.key(), path( 1 ), TestCall { sequence: 1, name: "a1-second" })
			.enqueue( caller_a.key(), path( 2 ), TestCall { sequence: 2, name: "a2" });
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
		let caller = PluginNode::new( Vec::new() );
		let older = PluginNode::new( Vec::new() );
		let newer = PluginNode::new( Vec::new() );
		let queues = HashMap::from([
			( newer.key(), DestinationQueue::default().enqueue(
				caller.key(), path( 1 ), TestCall { sequence: 2, name: "newer" },
			)),
			( older.key(), DestinationQueue::default().enqueue(
				caller.key(), path( 2 ), TestCall { sequence: 1, name: "older" },
			)),
		]);
		assert_eq!( oldest_destination( &queues ), Some( older.key() ));
	}

	#[test]
	fn execution_depth_follows_the_calling_plugin() {
		let mut context = TestContext { resources: ResourceTable::new() };
		let _ = context.resource_table();
		let caller = PluginNode::new( Vec::new() );
		let scheduler = AsyncScheduler::<TestContext>::new( HashMap::from([( caller.key(), 2 )]) );
		let root = scheduler.root_path();
		let child = scheduler.execution_path( caller.key() );
		assert_eq!( root.depth, 0 );
		assert_eq!( child.depth, 3 );
	}
}
