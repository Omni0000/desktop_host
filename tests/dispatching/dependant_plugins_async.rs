use std::collections::{ HashMap, HashSet, VecDeque };
use std::future::Future ;
use std::sync::{ Arc, Mutex as StdMutex, atomic::{ AtomicUsize, Ordering }};
use std::task::{ Context, Poll };
use wasm_link::{
	Binding, Component, Engine, Function, FunctionKind, Interface, Linker, Plugin,
	PluginInstanceAsync, ResourceTable, ReturnKind, Val,
};
use wasm_link::cardinality::{ Any, ExactlyOne };
use crate::fixture_linking::TestContext ;

fixtures! {
	bindings = { root: "root", dependency: "dependency" };
	plugins  = { startup: "startup", child: "child", child_sync: "child-sync" };
}

const SUSPENDING_COMPONENT: &str = r#"(component
	(type $host (instance
		(type $wait (func async (result u32)))
		(export "wait" (func (type $wait)))
	))
	(import "test:host/root" (instance $host (type $host)))
	(alias export $host "wait" (func $wait))
	(core module $memory (memory (export "memory") 1))
	(core instance $memory (instantiate $memory))
	(core func $lowered-wait (canon lower (func $wait) async (memory $memory "memory")))
	(core func $waitable-set-new (canon waitable-set.new))
	(core func $waitable-join (canon waitable.join))
	(core func $task-return (canon task.return (result u32)))
	(core func $task-cancel (canon task.cancel))
	(core module $implementation
		(import "" "memory" (memory 1))
		(import "" "wait" (func $wait (param i32) (result i32)))
		(import "" "waitable-set-new" (func $waitable-set-new (result i32)))
		(import "" "waitable-join" (func $waitable-join (param i32 i32)))
		(import "" "task-return" (func $task-return (param i32)))
		(import "" "task-cancel" (func $task-cancel))
		(func (export "get-value") (result i32)
			(local $status i32) (local $waitable-set i32)
			(local.set $status (call $wait (i32.const 0)))
			(if (i32.ne (i32.and (local.get $status) (i32.const 15)) (i32.const 1))
				(then unreachable))
			(local.set $waitable-set (call $waitable-set-new))
			(call $waitable-join
				(i32.shr_u (local.get $status) (i32.const 4))
				(local.get $waitable-set))
			(i32.or (i32.const 2) (i32.shl (local.get $waitable-set) (i32.const 4)))
		)
		(func (export "callback") (param i32 i32 i32) (result i32)
			(if (i32.eq (local.get 0) (i32.const 6))
				(then (call $task-cancel))
				(else (call $task-return (i32.const 42)))
			)
			i32.const 0
		)
	)
	(core instance $implementation-instance (instantiate $implementation
		(with "" (instance
			(export "memory" (memory $memory "memory"))
			(export "wait" (func $lowered-wait))
			(export "waitable-set-new" (func $waitable-set-new))
			(export "waitable-join" (func $waitable-join))
			(export "task-return" (func $task-return))
			(export "task-cancel" (func $task-cancel))
		))
	))
	(alias core export $implementation-instance "get-value" (core func $get-value))
	(alias core export $implementation-instance "callback" (core func $callback))
	(func $lifted-get-value async (result u32) (canon lift
		(core func $get-value)
		async
		(memory $memory "memory")
		(callback (func $callback))
	))
	(instance $root (export "get-value" (func $lifted-get-value)))
	(export "test:plugin/root" (instance $root))
)"#;

fn suspending_interface() -> Interface {
	Interface::new(
		HashMap::from([(
			"get-value".to_string(),
			Function::new( FunctionKind::Freestanding, ReturnKind::AssumeNoResources ),
		)]),
		HashSet::new(),
	)
}

#[test]
fn link_async_accepts_a_sync_socket_binding() {
	futures::executor::block_on( async {
		let engine = Engine::default();
		let linker = Linker::new( &engine );
		let plugins = fixtures::plugins( &engine );
		let bindings = fixtures::bindings();
		let child = plugins.child_sync.plugin.instantiate( &engine, &linker )
			.expect( "Failed to instantiate sync child" );
		let dependency = Binding::new(
			bindings.dependency.package,
			HashMap::from([( bindings.dependency.name, bindings.dependency.spec )]),
			ExactlyOne( "_".to_string(), child ),
		);
		let startup = plugins.startup.plugin
			.link_async( &engine, linker, vec![ dependency ])
			.await
			.expect( "Failed to link async plugin to sync socket" );
		let root = Binding::new(
			bindings.root.package,
			HashMap::from([( bindings.root.name, bindings.root.spec )]),
			ExactlyOne( "_".to_string(), startup ),
		);

		assert!( matches!(
			root.dispatch_async( "root", "get-primitive", &[] ).await,
			Ok( ExactlyOne( _, Ok( Val::U32( 42 ))))
		));
	});
}

#[test]
fn async_binding_accepts_a_sync_plugin_instance() {
	futures::executor::block_on( async {
		let engine = Engine::default();
		let linker = Linker::new( &engine );
		let plugins = fixtures::plugins( &engine );
		let bindings = fixtures::bindings();
		let child = plugins.child_sync.plugin.instantiate( &engine, &linker )
			.expect( "Failed to instantiate sync child" );
		let child: PluginInstanceAsync<TestContext> = child.into();
		let dependency = Binding::new(
			bindings.dependency.package,
			HashMap::from([( bindings.dependency.name, bindings.dependency.spec )]),
			ExactlyOne( "_".to_string(), child ),
		);
		let startup = plugins.startup.plugin
			.link_async( &engine, linker, vec![ dependency ])
			.await
			.expect( "Failed to link through async binding" );
		let root = Binding::new(
			bindings.root.package,
			HashMap::from([( bindings.root.name, bindings.root.spec )]),
			ExactlyOne( "_".to_string(), startup ),
		);

		assert!( matches!(
			root.dispatch_async( "root", "get-primitive", &[] ).await,
			Ok( ExactlyOne( _, Ok( Val::U32( 42 ))))
		));
	});
}

#[test]
fn links_and_dispatches_wit_async_across_plugin_stores_on_one_worker() {
	futures::executor::block_on( async {
		let engine = Engine::default();
		let linker = Linker::new( &engine );
		let plugins = fixtures::plugins( &engine );
		let bindings = fixtures::bindings();

		let child_instance = plugins.child.plugin
			.instantiate_async( &engine, &linker )
			.await
			.expect( "Failed to instantiate child plugin asynchronously" );
		let dependency_binding = Binding::new(
			bindings.dependency.package,
			HashMap::from([( bindings.dependency.name, bindings.dependency.spec )]),
			ExactlyOne( "_".to_string(), child_instance ),
		);

		let startup_instance = plugins.startup.plugin
			.link_async( &engine, linker, vec![ dependency_binding ] )
			.await
			.expect( "Failed to link startup plugin asynchronously" );
		let root_binding = Binding::new(
			bindings.root.package,
			HashMap::from([( bindings.root.name, bindings.root.spec )]),
			ExactlyOne( "_".to_string(), startup_instance ),
		);

		match root_binding.dispatch_async( "root", "get-primitive", &[] ).await {
			Ok( ExactlyOne( _, Ok( Val::U32( 42 )))) => {}
			value => panic!( "Expected Ok( ExactlyOne( Ok( U32( 42 )))), found: {:#?}", value ),
		}

		let ( first, second ) = futures::join!(
			root_binding.dispatch_async( "root", "get-primitive", &[] ),
			root_binding.dispatch_async( "root", "get-primitive", &[] ),
		);
		for value in [ first, second ] {
			match value {
				Ok( ExactlyOne( _, Ok( Val::U32( 42 )))) => {}
				value => panic!( "Expected queued async dispatch to return U32(42), found: {:#?}", value ),
			}
		}
	});
}

#[test]
fn dropping_a_dispatch_releases_the_plugin_for_the_next_session() {
	futures::executor::block_on( async {
		let engine = Engine::default();
		let mut linker = Linker::new( &engine );
		let calls = Arc::new( AtomicUsize::new( 0 ));
		let host_calls = Arc::clone( &calls );
		let ( release_first, first ) = futures::channel::oneshot::channel();
		let ( release_second, second ) = futures::channel::oneshot::channel();
		let ( release_third, third ) = futures::channel::oneshot::channel();
		let ( release_fourth, fourth ) = futures::channel::oneshot::channel();
		let suspended = Arc::new( StdMutex::new( VecDeque::from([ first, second, third, fourth ])));
		let host_suspended = Arc::clone( &suspended );
		linker.root().instance( "test:host/root" ).unwrap()
			.func_new_concurrent( "wait", move | _, _, _, results | {
				host_calls.fetch_add( 1, Ordering::SeqCst );
				let suspended = host_suspended.lock().unwrap().pop_front().unwrap();
				Box::pin( async move {
					let _ = suspended.await;
					results[0] = Val::U32( 42 );
					Ok(())
				})
			})
			.unwrap();

		let component = Component::new( &engine, SUSPENDING_COMPONENT ).unwrap();
		let instance = Plugin::new(
			component,
			TestContext { resource_table: ResourceTable::new() },
		).instantiate_async( &engine, &linker ).await.unwrap();
		let binding = Binding::new(
			"test:plugin",
			HashMap::from([(
				"root".to_string(),
				suspending_interface(),
			)]),
			ExactlyOne( "plugin".to_string(), instance ),
		);

		let mut first = Box::pin( binding.dispatch_async( "root", "get-value", &[] ));
		let waker = futures::task::noop_waker();
		let mut context = Context::from_waker( &waker );
		for _ in 0..10 {
			assert!( matches!( first.as_mut().poll( &mut context ), Poll::Pending ));
			if calls.load( Ordering::SeqCst ) == 1 { break; }
		}
		assert_eq!( calls.load( Ordering::SeqCst ), 1 );
		drop( first );
		let _ = release_first.send(());
		let mut second = Box::pin( binding.dispatch_async( "root", "get-value", &[] ));
		for _ in 0..10 {
			assert!( matches!( second.as_mut().poll( &mut context ), Poll::Pending ));
			if calls.load( Ordering::SeqCst ) == 2 { break; }
		}
		assert_eq!( calls.load( Ordering::SeqCst ), 2 );
		let _ = release_second.send(());
		let second = second.await;
		assert!( matches!(
			second,
			Ok( ExactlyOne( _, Ok( Val::U32( 42 ))))
		), "unexpected second dispatch: {second:#?}" );
		assert_eq!( calls.load( Ordering::SeqCst ), 2 );

		let mut third = Box::pin( binding.dispatch_async( "root", "get-value", &[] ));
		for _ in 0..10 {
			assert!( matches!( third.as_mut().poll( &mut context ), Poll::Pending ));
			if calls.load( Ordering::SeqCst ) == 3 { break; }
		}
		assert_eq!( calls.load( Ordering::SeqCst ), 3 );
		let mut cancelled_waiter = Box::pin( binding.dispatch_async( "root", "get-value", &[] ));
		for _ in 0..3 {
			assert!( matches!( cancelled_waiter.as_mut().poll( &mut context ), Poll::Pending ));
		}
		drop( cancelled_waiter );
		let _ = release_third.send(());
		assert!( matches!( third.await, Ok( ExactlyOne( _, Ok( Val::U32( 42 ))))));

		let mut fourth = Box::pin( binding.dispatch_async( "root", "get-value", &[] ));
		for _ in 0..10 {
			assert!( matches!( fourth.as_mut().poll( &mut context ), Poll::Pending ));
			if calls.load( Ordering::SeqCst ) == 4 { break; }
		}
		assert_eq!( calls.load( Ordering::SeqCst ), 4 );
		let _ = release_fourth.send(());
		assert!( matches!( fourth.await, Ok( ExactlyOne( _, Ok( Val::U32( 42 ))))));
	});
}

#[test]
fn one_session_keeps_multiple_calls_to_a_shared_plugin_suspended() {
	futures::executor::block_on( async {
		let engine = Engine::default();
		let mut linker = Linker::new( &engine );
		let calls = Arc::new( AtomicUsize::new( 0 ));
		let host_calls = Arc::clone( &calls );
		let ( release_first, first ) = futures::channel::oneshot::channel();
		let ( release_second, second ) = futures::channel::oneshot::channel();
		let suspended = Arc::new( StdMutex::new( VecDeque::from([ first, second ])));
		let host_suspended = Arc::clone( &suspended );
		linker.root().instance( "test:host/root" ).unwrap()
			.func_new_concurrent( "wait", move | _, _, _, results | {
				host_calls.fetch_add( 1, Ordering::SeqCst );
				let suspended = host_suspended.lock().unwrap().pop_front().unwrap();
				Box::pin( async move {
					let _ = suspended.await;
					results[0] = Val::U32( 42 );
					Ok(())
				})
			})
			.unwrap();
		let instance = Plugin::new(
			Component::new( &engine, SUSPENDING_COMPONENT ).unwrap(),
			TestContext { resource_table: ResourceTable::new() },
		).instantiate_async( &engine, &linker ).await.unwrap();
		let binding = Binding::new(
			"test:plugin",
			HashMap::from([( "root".to_string(), suspending_interface() )]),
			Any( HashMap::from([
				( "first".to_string(), instance.clone() ),
				( "second".to_string(), instance ),
			])),
		);

		let mut dispatch = Box::pin( binding.dispatch_async( "root", "get-value", &[] ));
		let waker = futures::task::noop_waker();
		let mut context = Context::from_waker( &waker );
		for _ in 0..20 {
			assert!( matches!( dispatch.as_mut().poll( &mut context ), Poll::Pending ));
			if calls.load( Ordering::SeqCst ) == 2 { break; }
		}
		assert_eq!( calls.load( Ordering::SeqCst ), 2 );
		let _ = release_first.send(());
		let _ = release_second.send(());
		let Any( results ) = dispatch.await.unwrap();
		assert_eq!( results.len(), 2 );
		assert!( results.values().all(| value | matches!( value, Ok( Val::U32( 42 )))));
	});
}
