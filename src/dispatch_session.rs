use std::cell::RefCell ;
use std::future::Future ;
use std::pin::Pin ;
use std::sync::{ Arc, Mutex as StdMutex };
use std::sync::atomic::{ AtomicBool, AtomicU64, Ordering };
use std::task::{ Context, Poll };

use futures::channel::oneshot ;
use futures::future::BoxFuture ;
use futures::stream::{ FuturesUnordered, Stream };
use futures::task::AtomicWaker ;


static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new( 1 );

thread_local! {
	static CURRENT_SESSION: RefCell<Option<Arc<SessionShared>>> = const { RefCell::new( None ) };
}

pub(crate) struct SessionShared {
	id: u64,
	cancelled: AtomicBool,
	pending: StdMutex<Vec<BoxFuture<'static, ()>>>,
	waker: AtomicWaker,
}

impl SessionShared {
	fn new() -> Arc<Self> {
		Arc::new( Self {
			id: NEXT_SESSION_ID.fetch_add( 1, Ordering::Relaxed ),
			cancelled: AtomicBool::new( false ),
			pending: StdMutex::new( Vec::new() ),
			waker: AtomicWaker::new(),
		})
	}

	pub(crate) fn id( &self ) -> u64 { self.id }
	pub(crate) fn is_cancelled( &self ) -> bool { self.cancelled.load( Ordering::Acquire ) }

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

pub(crate) async fn run<R, F>( future: F ) -> R
where
	R: Send + 'static,
	F: Future<Output = R> + Send + 'static,
{
	let shared = SessionShared::new();
	let ( response, result ) = oneshot::channel();
	shared.spawn( Box::pin( async move {
		let _ = response.send( future.await );
	}));
	SessionFuture {
		shared,
		tasks: FuturesUnordered::new(),
		result,
	}.await
}

struct SessionFuture<R> {
	shared: Arc<SessionShared>,
	tasks: FuturesUnordered<BoxFuture<'static, ()>>,
	result: oneshot::Receiver<R>,
}

impl<R> Drop for SessionFuture<R> {
	fn drop( &mut self ) {
		self.shared.cancelled.store( true, Ordering::Release );
	}
}

impl<R> Future for SessionFuture<R> {
	type Output = R;

	fn poll( mut self: Pin<&mut Self>, cx: &mut Context<'_> ) -> Poll<Self::Output> {
		self.shared.waker.register( cx.waker() );
		let pending = self.shared.take_pending();
		self.tasks.extend( pending );

		let _session = CurrentSessionGuard::enter( &self.shared );

		let result = match Pin::new( &mut self.result ).poll( cx ) {
			Poll::Ready( Ok( result )) => Poll::Ready( result ),
			Poll::Ready( Err( _ )) => panic!( "dispatch session root task ended without a response" ),
			Poll::Pending => {
				while let Poll::Ready( Some(())) = Pin::new( &mut self.tasks ).poll_next( cx ) {}
				match Pin::new( &mut self.result ).poll( cx ) {
					Poll::Ready( Ok( result )) => Poll::Ready( result ),
					Poll::Ready( Err( _ )) => panic!( "dispatch session root task ended without a response" ),
					Poll::Pending => Poll::Pending,
				}
			}
		};

		result
	}
}
