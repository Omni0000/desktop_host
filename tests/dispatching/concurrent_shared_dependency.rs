use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{ AtomicUsize, Ordering };
use std::task::{ Context, Poll };

use futures::task::{ AtomicWaker, noop_waker_ref };
use wasm_link::{ Binding, Engine, Linker, Val };
use wasm_link::cardinality::{ Any, ExactlyOne };

fixtures! {
	bindings = { branch: "branch", shared: "shared" };
	plugins = {
		branch_b: "branch-b",
		branch_c: "branch-c",
		shared: "shared",
	};
}

struct Gate {
	arrivals: AtomicUsize,
	waker: AtomicWaker,
}

impl Gate {
	fn new() -> Arc<Self> {
		Arc::new( Self {
			arrivals: AtomicUsize::new( 0 ),
			waker: AtomicWaker::new(),
		})
	}

	async fn arrive( &self ) {
		if self.arrivals.fetch_add( 1, Ordering::AcqRel ) + 1 >= 2 {
			self.waker.wake();
		}
		futures::future::poll_fn(| cx | {
			self.waker.register( cx.waker() );
			match self.arrivals.load( Ordering::Acquire ) >= 2 {
				true => Poll::Ready(()),
				false => Poll::Pending,
			}
		}).await;
	}
}

#[test]
fn shared_async_destination_runs_both_suspended_calls() {
	futures::executor::block_on( async {
		let engine = Engine::default();
		let mut linker = Linker::new( &engine );
		let gate = Gate::new();
		let linked_gate = Arc::clone( &gate );
		linker.root()
			.instance( "test:gate/root" )
			.and_then(| mut instance | instance.func_new_concurrent(
				"wait",
				move | _ctx, _ty, _args, results | {
					let gate = Arc::clone( &linked_gate );
					Box::pin( async move {
						gate.arrive().await;
						results[0] = Val::U32( 1 );
						Ok(())
					})
				},
			))
			.expect( "failed to link the async gate" );

		let plugins = fixtures::plugins( &engine );
		let bindings = fixtures::bindings();
		let shared = plugins.shared.plugin
			.instantiate_async( &engine, &linker ).await
			.expect( "failed to instantiate the shared plugin" );
		let shared_binding = Binding::new(
			bindings.shared.package,
			HashMap::from([( bindings.shared.name, bindings.shared.spec )]),
			ExactlyOne( "shared".to_string(), shared ),
		);

		let branch_b = plugins.branch_b.plugin
			.link_async( &engine, linker.clone(), vec![ shared_binding.clone() ]).await
			.expect( "failed to link branch B" );
		let branch_c = plugins.branch_c.plugin
			.link_async( &engine, linker.clone(), vec![ shared_binding ]).await
			.expect( "failed to link branch C" );
		let branches = Binding::new(
			bindings.branch.package,
			HashMap::from([( bindings.branch.name, bindings.branch.spec )]),
			Any( HashMap::from([
				( "b".to_string(), branch_b ),
				( "c".to_string(), branch_c ),
			])),
		);

		let mut dispatch = std::pin::pin!( branches.dispatch( "root", "run", &[] ));
		let mut context = Context::from_waker( noop_waker_ref() );
		let result = ( 0..100 ).find_map(| _ | match dispatch.as_mut().poll( &mut context ) {
			Poll::Ready( result ) => Some( result ),
			Poll::Pending => None,
		});
		assert!( matches!(
			result,
			Some( Ok( Any( ref values )))
				if matches!( values.get( "b" ), Some( Ok( Val::U32( 1 ))))
					&& matches!( values.get( "c" ), Some( Ok( Val::U32( 1 ))))
		), "unexpected dispatch result: {result:#?}; arrivals: {}", gate.arrivals.load( Ordering::Acquire ));
	});
}

#[test]
fn cancelling_a_suspended_dispatch_releases_the_destination() {
	futures::executor::block_on( async {
		let engine = Engine::default();
		let mut linker = Linker::new( &engine );
		let gate = Gate::new();
		let linked_gate = Arc::clone( &gate );
		linker.root()
			.instance( "test:gate/root" )
			.and_then(| mut instance | instance.func_new_concurrent(
				"wait",
				move | _ctx, _ty, _args, results | {
					let gate = Arc::clone( &linked_gate );
					Box::pin( async move {
						gate.arrive().await;
						results[0] = Val::U32( 1 );
						Ok(())
					})
				},
			))
			.expect( "failed to link the async gate" );

		let plugins = fixtures::plugins( &engine );
		let bindings = fixtures::bindings();
		let shared = plugins.shared.plugin
			.instantiate_async( &engine, &linker ).await
			.expect( "failed to instantiate the shared plugin" );
		let binding = Binding::new(
			bindings.shared.package,
			HashMap::from([( bindings.shared.name, bindings.shared.spec )]),
			ExactlyOne( "shared".to_string(), shared ),
		);

		let mut first = Box::pin( binding.dispatch( "root", "wait", &[] ));
		let mut context = Context::from_waker( noop_waker_ref() );
		assert!( matches!( first.as_mut().poll( &mut context ), Poll::Pending ));
		drop( first );

		let mut second = Box::pin( binding.dispatch( "root", "wait", &[] ));
		let result = ( 0..100 ).find_map(| _ | match second.as_mut().poll( &mut context ) {
			Poll::Ready( result ) => Some( result ),
			Poll::Pending => None,
		});
		assert!( matches!(
			result,
			Some( Ok( ExactlyOne( _, Ok( Val::U32( 1 )))))
		), "unexpected dispatch result after cancellation: {result:#?}" );
	});
}
