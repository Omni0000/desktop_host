use std::collections::{ HashMap, HashSet, VecDeque };
use std::future::Future ;
use std::sync::{ Arc, Mutex as StdMutex, atomic::{ AtomicUsize, Ordering }};
use std::task::{ Context, Poll };
use wasm_link::{ Binding, Engine, Function, FunctionKind, Interface, Linker, PluginInstanceAsync, ReturnKind, Val };
use wasm_link::cardinality::{ Any, ExactlyOne };
use crate::fixture_linking::TestContext ;

fixtures! {
	bindings = { root: "root", dependency: "dependency" };
	plugins  = {
		startup: "startup",
		child: "child",
		child_sync: "child-sync",
		suspending: "suspending",
		sync_import: "sync-import",
	};
}

#[derive( Debug, thiserror::Error )]
enum TestHostError {
	#[error( "host wait queue lock was poisoned" )] QueuePoisoned,
	#[error( "host received more wait calls than the test prepared" )] UnexpectedWait,
	#[error( "host wait release was dropped" )] ReleaseDropped,
}

fn test_engine() -> Result<Engine, wasmtime::Error> {
	let mut config = wasmtime::Config::new();
	config.wasm_component_model_implements( true );
	Engine::new( &config )
}

fn suspending_interface() -> Interface {
	Interface::new(
		HashMap::from([(
			"get-value".to_string(),
			Function::new( FunctionKind::Freestanding, ReturnKind::AssumeNoResources ),
		)]),
		HashSet::new(),
	)
}

type SingleDispatch = Result<ExactlyOne<String, Result<Val, wasm_link::DispatchError>>, wasm_link::DispatchError>;

fn assert_u32( result: &SingleDispatch, expected: u32 ) {
	assert!(
		matches!( &result, Ok( ExactlyOne( _, Ok( Val::U32( value )))) if *value == expected ),
		"unexpected dispatch result: {result:#?}",
	);
}

fn poll_until_calls<F: Future>( mut future: std::pin::Pin<&mut F>, calls: &AtomicUsize, expected: usize ) {
	let waker = futures::task::noop_waker();
	let mut context = Context::from_waker( &waker );
	for _ in 0..100 {
		assert!( matches!( future.as_mut().poll( &mut context ), Poll::Pending ));
		if calls.load( Ordering::SeqCst ) == expected { return; }
	}
	assert_eq!( calls.load( Ordering::SeqCst ), expected, "host wait call count" );
}

fn suspending_linker(
	engine: &Engine,
	waits: VecDeque<futures::channel::oneshot::Receiver<()>>,
	calls: Arc<AtomicUsize>,
) -> Result<Linker<TestContext>, wasmtime::Error> {
	let waits = Arc::new( StdMutex::new( waits ));
	let host_waits = Arc::clone( &waits );
	let mut linker = Linker::new( engine );
	linker.root().instance( "test:host/root" )?
		.func_new_concurrent( "wait", move | _, _, _, results | {
			calls.fetch_add( 1, Ordering::SeqCst );
			let wait = host_waits.lock()
				.map_err(| _ | TestHostError::QueuePoisoned )
				.and_then(| mut waits | waits.pop_front().ok_or( TestHostError::UnexpectedWait ));
			Box::pin( async move {
				wait?.await.map_err(| _ | TestHostError::ReleaseDropped )?;
				results[0] = Val::U32( 42 );
				Ok(())
			})
		})?;
	Ok( linker )
}

#[test]
fn link_async_accepts_a_sync_socket_binding() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let engine = test_engine()?;
		let linker = Linker::new( &engine );
		let plugins = fixtures::plugins( &engine );
		let bindings = fixtures::bindings();
		let child = plugins.child_sync.plugin.instantiate( &engine, &linker )?;
		let dependency = Binding::new(
			bindings.dependency.package,
			HashMap::from([( bindings.dependency.name, bindings.dependency.spec )]),
			ExactlyOne( "_".to_string(), child ),
		);
		let startup = plugins.startup.plugin.link_async( &engine, linker, vec![ dependency ]).await?;
		let root = Binding::new(
			bindings.root.package,
			HashMap::from([( bindings.root.name, bindings.root.spec )]),
			ExactlyOne( "_".to_string(), startup ),
		);
		assert_u32( &root.dispatch( "root", "get-primitive", &[] ).await, 42 );
		Ok(())
	})
}

#[test]
fn async_binding_accepts_a_sync_plugin_instance() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let engine = test_engine()?;
		let linker = Linker::new( &engine );
		let plugins = fixtures::plugins( &engine );
		let bindings = fixtures::bindings();
		let child = plugins.child_sync.plugin.instantiate( &engine, &linker )?;
		let child: PluginInstanceAsync<TestContext> = child.into();
		let dependency = Binding::new(
			bindings.dependency.package,
			HashMap::from([( bindings.dependency.name, bindings.dependency.spec )]),
			ExactlyOne( "_".to_string(), child ),
		);
		let startup = plugins.startup.plugin.link_async( &engine, linker, vec![ dependency ]).await?;
		let root = Binding::new(
			bindings.root.package,
			HashMap::from([( bindings.root.name, bindings.root.spec )]),
			ExactlyOne( "_".to_string(), startup ),
		);
		assert_u32( &root.dispatch( "root", "get-primitive", &[] ).await, 42 );
		Ok(())
	})
}

#[test]
fn a_shared_async_dependency_remains_dispatchable_across_sessions() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let engine = test_engine()?;
		let plugins = fixtures::plugins( &engine );
		let bindings = fixtures::bindings();
		let child = plugins.child.plugin.instantiate_async( &engine, &Linker::new( &engine )).await?;
		let dependency = Binding::new(
			bindings.dependency.package,
			HashMap::from([( bindings.dependency.name, bindings.dependency.spec )]),
			ExactlyOne( "_".to_string(), child ),
		);
		let first = plugins.startup.plugin
			.link_async( &engine, Linker::new( &engine ), vec![ dependency.clone() ]).await?;
		let root = Binding::new(
			bindings.root.package,
			HashMap::from([( bindings.root.name, bindings.root.spec )]),
			ExactlyOne( "_".to_string(), first ),
		);
		assert_u32( &root.dispatch( "root", "get-primitive", &[] ).await, 42 );

		let plugins = fixtures::plugins( &engine );
		let bindings = fixtures::bindings();
		let second = plugins.startup.plugin
			.link_async( &engine, Linker::new( &engine ), vec![ dependency ]).await?;
		let root = Binding::new(
			bindings.root.package,
			HashMap::from([( bindings.root.name, bindings.root.spec )]),
			ExactlyOne( "_".to_string(), second ),
		);
		assert_u32( &root.dispatch( "root", "get-primitive", &[] ).await, 42 );
		Ok(())
	})
}

#[test]
fn sync_import_accepts_an_async_export() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let engine = test_engine()?;
		let plugins = fixtures::plugins( &engine );
		let bindings = fixtures::bindings();
		let child = plugins.child.plugin.instantiate_async( &engine, &Linker::new( &engine )).await?;
		let dependency = Binding::new(
			bindings.dependency.package,
			HashMap::from([( bindings.dependency.name, bindings.dependency.spec )]),
			ExactlyOne( "_".to_string(), child ),
		);
		let _ = plugins.sync_import.plugin
			.link_async( &engine, Linker::new( &engine ), vec![ dependency ]).await?;
		Ok(())
	})
}

#[test]
fn async_linking_does_not_infer_effects_from_sync_instance_exports() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let engine = test_engine()?;
		let plugins = fixtures::plugins( &engine );
		let bindings = fixtures::bindings();
		let child = plugins.child.plugin.instantiate( &engine, &Linker::new( &engine ))?;
		let dependency = Binding::new(
			bindings.dependency.package,
			HashMap::from([( bindings.dependency.name, bindings.dependency.spec )]),
			ExactlyOne( "_".to_string(), child ),
		);
		let _ = plugins.startup.plugin
			.link_async( &engine, Linker::new( &engine ), vec![ dependency ]).await?;
		Ok(())
	})
}

#[test]
fn independent_sessions_sharing_a_graph_are_serialized() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let engine = test_engine()?;
		let calls = Arc::new( AtomicUsize::new( 0 ));
		let ( release_first, first_wait ) = futures::channel::oneshot::channel();
		let ( release_second, second_wait ) = futures::channel::oneshot::channel();
		let linker = suspending_linker(
			&engine,
			VecDeque::from([ first_wait, second_wait ]),
			Arc::clone( &calls ),
		)?;
		let instance = fixtures::plugins( &engine ).suspending.plugin
			.instantiate_async( &engine, &linker ).await?;
		let binding = Binding::new(
			"test:plugin",
			HashMap::from([( "root".to_string(), suspending_interface() )]),
			ExactlyOne( "plugin".to_string(), instance ),
		);
		let mut first = Box::pin( binding.dispatch( "root", "get-value", &[] ));
		let mut second = Box::pin( binding.dispatch( "root", "get-value", &[] ));
		poll_until_calls( first.as_mut(), &calls, 1 );
		let waker = futures::task::noop_waker();
		let mut context = Context::from_waker( &waker );
		assert!( second.as_mut().poll( &mut context ).is_pending() );
		assert_eq!( calls.load( Ordering::SeqCst ), 1, "second session entered the busy plugin" );
		assert!( release_first.send(()).is_ok(), "first host wait was dropped" );
		assert_u32( &first.await, 42 );
		poll_until_calls( second.as_mut(), &calls, 2 );
		assert!( release_second.send(()).is_ok(), "second host wait was dropped" );
		assert_u32( &second.await, 42 );
		Ok(())
	})
}

#[test]
fn dropping_a_dispatch_releases_the_graph_for_the_next_session() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let engine = test_engine()?;
		let calls = Arc::new( AtomicUsize::new( 0 ));
		let ( release_first, first ) = futures::channel::oneshot::channel();
		let ( release_second, second ) = futures::channel::oneshot::channel();
		let linker = suspending_linker(
			&engine,
			VecDeque::from([ first, second ]),
			Arc::clone( &calls ),
		)?;
		let instance = fixtures::plugins( &engine ).suspending.plugin
			.instantiate_async( &engine, &linker ).await?;
		let binding = Binding::new(
			"test:plugin",
			HashMap::from([( "root".to_string(), suspending_interface() )]),
			ExactlyOne( "plugin".to_string(), instance ),
		);

		let mut first = Box::pin( binding.dispatch( "root", "get-value", &[] ));
		poll_until_calls( first.as_mut(), &calls, 1 );
		drop( first );
		assert!( release_first.send(()).is_ok(), "first host wait was dropped" );

		let mut second = Box::pin( binding.dispatch( "root", "get-value", &[] ));
		poll_until_calls( second.as_mut(), &calls, 2 );
		assert!( release_second.send(()).is_ok(), "second host wait was dropped" );
		assert_u32( &second.await, 42 );
		Ok(())
	})
}

#[test]
fn one_session_keeps_multiple_plugin_calls_suspended() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let engine = test_engine()?;
		let calls = Arc::new( AtomicUsize::new( 0 ));
		let ( release_first, first ) = futures::channel::oneshot::channel();
		let ( release_second, second ) = futures::channel::oneshot::channel();
		let linker = suspending_linker(
			&engine,
			VecDeque::from([ first, second ]),
			Arc::clone( &calls ),
		)?;
		let first_instance = fixtures::plugins( &engine ).suspending.plugin
			.instantiate_async( &engine, &linker ).await?;
		let second_instance = fixtures::plugins( &engine ).suspending.plugin
			.instantiate_async( &engine, &linker ).await?;
		let binding = Binding::new(
			"test:plugin",
			HashMap::from([( "root".to_string(), suspending_interface() )]),
			Any( HashMap::from([
				( "first".to_string(), first_instance ),
				( "second".to_string(), second_instance ),
			])),
		);

		let mut dispatch = Box::pin( binding.dispatch( "root", "get-value", &[] ));
		poll_until_calls( dispatch.as_mut(), &calls, 2 );
		assert!( release_first.send(()).is_ok(), "first host wait was dropped" );
		assert!( release_second.send(()).is_ok(), "second host wait was dropped" );
		let Any( results ) = dispatch.await?;
		assert_eq!( results.len(), 2 );
		assert!( results.values().all(| value | matches!( value, Ok( Val::U32( 42 )))));
		Ok(())
	})
}
