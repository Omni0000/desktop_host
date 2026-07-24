use std::collections::HashMap;
use std::sync::{ Arc, Condvar, Mutex };
use wasm_link::{ Binding, Engine, Linker, SocketBindingAny, Val };
use wasm_link::cardinality::ExactlyOne ;

fixtures! {
	bindings = { root: "root", dependency: "dependency" };
	plugins  = { startup: "startup", child: "child", gated_child: "gated-child" };
}

#[derive( Default )]
struct CallGate {
	entered: ( Mutex<bool>, Condvar ),
	released: ( Mutex<bool>, Condvar ),
}

impl CallGate {
	fn wait( &self ) {
		let mut entered = self.entered.0.lock().expect( "entry gate lock poisoned" );
		*entered = true;
		self.entered.1.notify_one();
		drop( entered );
		let released = self.released.0.lock().expect( "release gate lock poisoned" );
		drop( self.released.1.wait_while( released, | released | !*released )
			.expect( "release gate lock poisoned" ));
	}

	fn wait_until_entered( &self ) {
		let entered = self.entered.0.lock().expect( "entry gate lock poisoned" );
		drop( self.entered.1.wait_while( entered, | entered | !*entered )
			.expect( "entry gate lock poisoned" ));
	}

	fn release( &self ) {
		*self.released.0.lock().expect( "release gate lock poisoned" ) = true;
		self.released.1.notify_all();
	}
}

#[test]
fn concurrent_sync_plugins_wait_for_their_shared_dependency() {
	let engine = Engine::default();
	let mut linker = Linker::new( &engine );
	let gate = Arc::new( CallGate::default() );
	let gate_for_import = Arc::clone( &gate );
	linker.root().instance( "test:gate/gate" )
		.expect( "Failed to define gate interface" )
		.func_new( "wait", move | _ctx, _ty, _args, _results | {
			gate_for_import.wait();
			Ok(())
		})
		.expect( "Failed to define gate function" );
	let plugins_a = fixtures::plugins( &engine );
	let plugins_b = fixtures::plugins( &engine );
	let bindings = fixtures::bindings();
	let child = plugins_a.gated_child.plugin
		.instantiate( &engine, &linker )
		.expect( "Failed to instantiate shared child plugin" );
	let dependency = Binding::new(
		bindings.dependency.package,
		HashMap::from([( bindings.dependency.name, bindings.dependency.spec )]),
		ExactlyOne( "child".to_string(), child ),
	);
	let startup_a = plugins_a.startup.plugin
		.link( &engine, linker.clone(), vec![ dependency.clone() ])
		.expect( "Failed to link first startup plugin" );
	let startup_b = plugins_b.startup.plugin
		.link( &engine, linker, vec![ dependency ])
		.expect( "Failed to link second startup plugin" );
	let root_a = Arc::new( Binding::new(
		bindings.root.package.clone(),
		HashMap::from([( bindings.root.name.clone(), bindings.root.spec.clone() )]),
		ExactlyOne( "startup-a".to_string(), startup_a ),
	));
	let root_b = Arc::new( Binding::new(
		bindings.root.package,
		HashMap::from([( bindings.root.name, bindings.root.spec )]),
		ExactlyOne( "startup-b".to_string(), startup_b ),
	));
	let first = std::thread::spawn( move || {
		root_a.dispatch( "root", "get-primitive", &[] )
	});
	gate.wait_until_entered();
	let ( second_started, observe_second ) = std::sync::mpsc::sync_channel( 0 );
	let second = std::thread::spawn( move || {
		second_started.send(()).expect( "Failed to signal second dispatch" );
		root_b.dispatch( "root", "get-primitive", &[] )
	});
	observe_second.recv().expect( "Failed to observe second dispatch" );
	gate.release();

	[ first, second ].into_iter().for_each(| call | match call.join().expect( "dispatch thread panicked" ) {
		Ok( ExactlyOne( _, Ok( Val::U32( 42 )))) => {}
		value => panic!( "shared dependency call failed: {value:#?}" ),
	});
}

#[test]
fn dispatch_test_dependant_plugins_expect_primitive() {

	let engine = Engine::default();
	let linker = Linker::new( &engine );
	let plugins = fixtures::plugins( &engine );
	let bindings = fixtures::bindings();

	let child_instance = plugins.child.plugin
		.instantiate( &engine, &linker )
		.expect( "Failed to instantiate child plugin" );
	let dependency_binding = Binding::new(
		bindings.dependency.package,
		HashMap::from([( bindings.dependency.name, bindings.dependency.spec )]),
		ExactlyOne( "_".to_string(), child_instance ),
	);

	let startup_instance = plugins.startup.plugin
		.link( &engine, linker.clone(), vec![ dependency_binding ])
		.expect( "Failed to link startup plugin" );
	let root_binding = Binding::new(
		bindings.root.package,
		HashMap::from([( bindings.root.name, bindings.root.spec )]),
		ExactlyOne( "_".to_string(), startup_instance ),
	);

	match root_binding.dispatch( "root", "get-primitive", &[] ) {
		Ok( ExactlyOne( _, Ok( Val::U32( 42 )))) => {}
		value => panic!( "Expected Ok( ExactlyOne( Ok( U32( 42 )))), found: {:#?}", value ),
	}

}

#[test]
fn dispatch_async_test_dependant_plugins_expect_primitive() {

	futures::executor::block_on( async {
		let engine = Engine::default();
		let linker = Linker::new( &engine );
		let plugins = fixtures::plugins( &engine );
		let bindings = fixtures::bindings();

		let child_instance = plugins.child.plugin
			.instantiate( &engine, &linker )
			.expect( "Failed to instantiate synchronous child plugin" );
		let dependency_binding = Binding::new(
			bindings.dependency.package,
			HashMap::from([( bindings.dependency.name, bindings.dependency.spec )]),
			ExactlyOne( "_".to_string(), child_instance ),
		);

		let startup_instance = plugins.startup.plugin
			.link_async( &engine, linker.clone(), vec![ dependency_binding ])
			.await
			.expect( "Failed to link startup plugin asynchronously" );
		let root_binding = Binding::new(
			bindings.root.package,
			HashMap::from([( bindings.root.name, bindings.root.spec )]),
			ExactlyOne( "_".to_string(), startup_instance ),
		);

		match root_binding.dispatch( "root", "get-primitive", &[] ).await {
			Ok( ExactlyOne( _, Ok( Val::U32( 42 )))) => {}
			value => panic!( "Expected Ok( ExactlyOne( Ok( U32( 42 )))), found: {:#?}", value ),
		}
	});

}

#[test]
fn async_link_accepts_heterogeneous_sync_and_async_sockets() {
	futures::executor::block_on( async {
		let engine = Engine::default();
		let linker = Linker::new( &engine );
		let sync_plugins = fixtures::plugins( &engine );
		let async_plugins = fixtures::plugins( &engine );
		let root_plugins = fixtures::plugins( &engine );
		let bindings = fixtures::bindings();

		let sync_child = sync_plugins.child.plugin
			.instantiate( &engine, &linker )
			.expect( "Failed to instantiate synchronous child plugin" );
		let async_child = async_plugins.child.plugin
			.instantiate_async( &engine, &linker )
			.await
			.expect( "Failed to instantiate asynchronous child plugin" );
		let sync_socket = Binding::new(
			bindings.dependency.package,
			HashMap::from([( bindings.dependency.name, bindings.dependency.spec )]),
			ExactlyOne( "sync".to_string(), sync_child ),
		);
		let async_socket = Binding::new(
			bindings.root.package.clone(),
			HashMap::from([( bindings.root.name.clone(), bindings.root.spec.clone() )]),
			ExactlyOne( "async".to_string(), async_child ),
		);
		let sockets: Vec<SocketBindingAny<String, _>> =
			vec![ sync_socket.into(), async_socket.into() ];

		let root = root_plugins.startup.plugin
			.link_async( &engine, linker, sockets )
			.await
			.expect( "Failed to link with heterogeneous sockets" );
		let root = Binding::new(
			bindings.root.package,
			HashMap::from([( bindings.root.name, bindings.root.spec )]),
			ExactlyOne( "root".to_string(), root ),
		);

		assert!( matches!(
			root.dispatch( "root", "get-primitive", &[] ).await,
			Ok( ExactlyOne( _, Ok( Val::U32( 42 ))))
		));
	});
}
