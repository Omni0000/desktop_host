use std::cell::RefCell ;
use std::future::Future ;
use std::pin::Pin ;
use std::sync::{ Arc, Mutex as StdMutex };
use std::sync::atomic::{ AtomicU64, Ordering };
use std::task::{ Context, Poll };

use futures::future::BoxFuture ;
use futures::lock::Mutex ;
use futures::stream::{ FuturesUnordered, Stream };
use futures::task::AtomicWaker ;


static NEXT_LOCK_ID: AtomicU64 = AtomicU64::new( 1 );

#[derive( Clone, Debug )]
pub(crate) struct SessionLock {
	id: u64,
	mutex: Arc<Mutex<()>>,
}

pub(crate) fn new_lock() -> SessionLock {
	SessionLock {
		id: NEXT_LOCK_ID.fetch_add( 1, Ordering::Relaxed ),
		mutex: Arc::new( Mutex::new(()) ),
	}
}

pub(crate) fn merge_locks( mut locks: Vec<SessionLock> ) -> Vec<SessionLock> {
	locks.sort_unstable_by_key(| lock | lock.id );
	locks.dedup_by_key(| lock | lock.id );
	locks
}

thread_local! {
	static CURRENT_SESSION: RefCell<Option<Arc<SessionShared>>> = const { RefCell::new( None ) };
}

pub(crate) struct SessionShared {
	pending: StdMutex<Vec<BoxFuture<'static, ()>>>,
	waker: AtomicWaker,
}

impl SessionShared {
	fn new() -> Arc<Self> {
		Arc::new( Self {
			pending: StdMutex::new( Vec::new() ),
			waker: AtomicWaker::new(),
		})
	}

	pub(crate) fn spawn( &self, task: BoxFuture<'static, ()> ) {
		match self.pending.lock() {
			Ok( mut pending ) => pending.push( task ),
			Err( poisoned ) => poisoned.into_inner().push( task ),
		}
		self.waker.wake();
	}

	fn take_pending( &self ) -> Vec<BoxFuture<'static, ()>> {
		match self.pending.lock() {
			Ok( mut pending ) => std::mem::take( &mut *pending ),
			Err( poisoned ) => std::mem::take( &mut *poisoned.into_inner() ),
		}
	}
}

pub(crate) fn current() -> Option<Arc<SessionShared>> {
	CURRENT_SESSION.with_borrow( Clone::clone )
}

struct CurrentSessionGuard( Option<Arc<SessionShared>> );

impl CurrentSessionGuard {
	fn enter( shared: &Arc<SessionShared> ) -> Self {
		Self( CURRENT_SESSION.with_borrow_mut(| current |
			current.replace( Arc::clone( shared ))
		))
	}
}

impl Drop for CurrentSessionGuard {
	fn drop( &mut self ) {
		CURRENT_SESSION.with_borrow_mut(| current | *current = self.0.take() );
	}
}

pub(crate) async fn run<R, F>( locks: Vec<SessionLock>, future: F ) -> R
where
	R: Send + 'static,
	F: Future<Output = R> + Send + 'static,
{
	let mut guards = Vec::with_capacity( locks.len() );
	for lock in locks {
		guards.push( lock.mutex.lock_owned().await );
	}
	let shared = SessionShared::new();
	let result = SessionFuture {
		shared,
		tasks: FuturesUnordered::new(),
		root: Box::pin( future ),
	}.await;
	drop( guards );
	result
}

struct SessionFuture<F> {
	shared: Arc<SessionShared>,
	tasks: FuturesUnordered<BoxFuture<'static, ()>>,
	root: Pin<Box<F>>,
}

impl<F: Future> Future for SessionFuture<F> {
	type Output = F::Output;

	fn poll( mut self: Pin<&mut Self>, cx: &mut Context<'_> ) -> Poll<Self::Output> {
		self.shared.waker.register( cx.waker() );
		let pending = self.shared.take_pending();
		self.tasks.extend( pending );

		let _session = CurrentSessionGuard::enter( &self.shared );

		match self.root.as_mut().poll( cx ) {
			Poll::Ready( result ) => Poll::Ready( result ),
			Poll::Pending => {
				while let Poll::Ready( Some(())) = Pin::new( &mut self.tasks ).poll_next( cx ) {}
				self.root.as_mut().poll( cx )
			}
		}
	}
}
