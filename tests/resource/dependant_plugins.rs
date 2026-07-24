use std::collections::HashMap;
use wasm_link::{ Binding, Engine, Linker, Val };
use wasm_link::cardinality::ExactlyOne ;

fixtures! {
	bindings = { root: "root", dependency: "dependency" };
	plugins  = { consumer: "consumer", counter: "counter" };
}

#[test]
fn resource_test_wrapper() {

	let engine = Engine::default();
	let linker = Linker::new( &engine );
	let plugins = fixtures::plugins( &engine );
	let bindings = fixtures::bindings();

	let counter_instance = plugins.counter.plugin
		.instantiate( &engine, &linker )
		.expect( "Failed to instantiate counter plugin" );
	let dependency_binding = Binding::new(
		bindings.dependency.package,
		HashMap::from([( bindings.dependency.name, bindings.dependency.spec )]),
		ExactlyOne( "_".to_string(), counter_instance ),
	);

	let consumer_instance = plugins.consumer.plugin
		.link( &engine, linker.clone(), vec![ dependency_binding ])
		.expect( "Failed to link consumer plugin" );
	let root_binding = Binding::new(
		bindings.root.package,
		HashMap::from([( bindings.root.name, bindings.root.spec )]),
		ExactlyOne( "_".to_string(), consumer_instance ),
	);

	match root_binding.dispatch( "root", "get-value", &[] ) {
		Ok( ExactlyOne( _, Ok( Val::U32( 42 )))) => {}
		Ok( ExactlyOne( _, Ok( val ))) => panic!( "Expected U32(42), got: {:#?}", val ),
		Ok( ExactlyOne( _, Err( err ))) => panic!( "Method call failed: {:?}", err ),
		value => panic!( "Expected Ok( ExactlyOne( Ok( U32( 42 )))), got: {:#?}", value ),
	}

}

#[test]
fn async_resource_test_wrapper() {
	futures::executor::block_on( async {
		let engine = Engine::default();
		let linker = Linker::new( &engine );
		let plugins = fixtures::plugins( &engine );
		let bindings = fixtures::bindings();

		let counter_instance = plugins.counter.plugin
			.instantiate_async( &engine, &linker )
			.await
			.expect( "Failed to instantiate counter plugin asynchronously" );
		let dependency_binding = Binding::new(
			bindings.dependency.package,
			HashMap::from([( bindings.dependency.name, bindings.dependency.spec )]),
			ExactlyOne( "_".to_string(), counter_instance ),
		);

		let consumer_instance = plugins.consumer.plugin
			.link_async( &engine, linker, vec![ dependency_binding ])
			.await
			.expect( "Failed to link consumer plugin asynchronously" );
		let root_binding = Binding::new(
			bindings.root.package,
			HashMap::from([( bindings.root.name, bindings.root.spec )]),
			ExactlyOne( "_".to_string(), consumer_instance ),
		);

		match root_binding.dispatch( "root", "get-value", &[] ).await {
			Ok( ExactlyOne( _, Ok( Val::U32( 42 )))) => {}
			Ok( ExactlyOne( _, Ok( val ))) => panic!( "Expected U32(42), got: {:#?}", val ),
			Ok( ExactlyOne( _, Err( err ))) => panic!( "Method call failed: {:?}", err ),
			value => panic!( "Expected Ok( ExactlyOne( Ok( U32( 42 )))), got: {:#?}", value ),
		}
	});
}
