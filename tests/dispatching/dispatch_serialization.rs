use std::collections::HashMap ;
use std::sync::Arc ;

use wasm_link::{ Binding, DispatchError, Engine, Linker, Val };
use wasm_link::cardinality::ExactlyOne ;

fixtures! {
	bindings = { root: "root", sync_root: "sync-root" };
	plugins  = { plugin: "plugin", sync_plugin: "sync-plugin" };
}

#[test]
fn async_dispatch_is_driven_by_calling_future_and_serialized() {
	futures::executor::block_on( async {
		let engine = Engine::default();
		let linker = Linker::new( &engine );
		let plugins = fixtures::plugins( &engine );
		let bindings = fixtures::bindings();
		let instance = plugins.plugin.plugin
			.instantiate_async( &engine, &linker ).await
			.expect( "failed to instantiate scheduler fixture" );
		let binding_a = Binding::new(
			bindings.root.package.clone(),
			HashMap::from([( bindings.root.name.clone(), bindings.root.spec.clone() )]),
			ExactlyOne( "plugin".to_string(), instance.clone() ),
		);
		let binding_b = Binding::new(
			bindings.root.package,
			HashMap::from([( bindings.root.name, bindings.root.spec )]),
			ExactlyOne( "plugin".to_string(), instance ),
		);

		let ( a1, a2, a3, b1 ) = futures::join!(
			binding_a.dispatch( "root", "run", &[ Val::U32( 1 ) ]),
			binding_a.dispatch( "root", "run", &[ Val::U32( 2 ) ]),
			binding_a.dispatch( "root", "run", &[ Val::U32( 3 ) ]),
			binding_b.dispatch( "root", "run", &[ Val::U32( 10 ) ]),
		);
		assert_result( a1, 1, 1 );
		assert_result( a2, 2, 2 );
		assert_result( a3, 3, 3 );
		assert_result( b1, 4, 10 );
	});
}

fn assert_result(
	result: Result<ExactlyOne<String, Result<Val, DispatchError>>, DispatchError>,
	sequence: u32,
	value: u32,
) {
	assert!( matches!(
		result,
		Ok( ExactlyOne( _, Ok( Val::Tuple( values ))))
			if values == vec![ Val::U32( sequence ), Val::U32( value ) ]
	));
}

#[test]
fn synchronous_calls_serialize_a_shared_destination() {
	let engine = Engine::default();
	let linker = Linker::new( &engine );
	let plugins = fixtures::plugins( &engine );
	let bindings = fixtures::bindings();
	let instance = plugins.sync_plugin.plugin
		.instantiate( &engine, &linker )
		.expect( "failed to instantiate sync scheduler fixture" );
	let binding_a = Arc::new( Binding::new(
		bindings.sync_root.package.clone(),
		HashMap::from([( bindings.sync_root.name.clone(), bindings.sync_root.spec.clone() )]),
		ExactlyOne( "plugin".to_string(), instance.clone() ),
	));
	let binding_b = Arc::new( Binding::new(
		bindings.sync_root.package,
		HashMap::from([( bindings.sync_root.name, bindings.sync_root.spec )]),
		ExactlyOne( "plugin".to_string(), instance ),
	));
	let start = Arc::new( std::sync::Barrier::new( 17 ));
	let threads = ( 0..16 ).map(| value | {
		let binding = match value % 2 { 0 => Arc::clone( &binding_a ), _ => Arc::clone( &binding_b )};
		let start = Arc::clone( &start );
		std::thread::spawn( move || {
			start.wait();
			binding.dispatch( "root", "run", &[ Val::U32( value ) ])
		})
	}).collect::<Vec<_>>();
	start.wait();

	let mut sequences = threads.into_iter().map(| thread | {
		let result = thread.join().expect( "sync dispatch thread panicked" );
		match result {
			Ok( ExactlyOne( _, Ok( Val::Tuple( values )))) => match values.as_slice() {
				[ Val::U32( sequence ), Val::U32( _ ) ] => *sequence,
				_ => panic!( "unexpected sync response values: {values:#?}" ),
			},
			value => panic!( "shared sync destination rejected a caller: {value:#?}" ),
		}
	}).collect::<Vec<_>>();
	sequences.sort_unstable();
	assert_eq!( sequences, ( 1..=16 ).collect::<Vec<_>>() );
}
