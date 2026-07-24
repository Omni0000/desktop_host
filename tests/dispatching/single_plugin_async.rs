use std::collections::HashMap;
use wasm_link::{ Binding, Engine, Linker, Val };
use wasm_link::cardinality::ExactlyOne ;

fixtures! {
	bindings = { root: "root" };
	plugins  = { plugin: "plugin" };
}

#[test]
fn instantiates_and_dispatches_wit_async_plugin() {
	futures::executor::block_on( async {
		let engine = Engine::default();
		let linker = Linker::new( &engine );
		let plugins = fixtures::plugins( &engine );
		let bindings = fixtures::bindings();

		let instance = plugins.plugin.plugin
			.instantiate_async( &engine, &linker )
			.await
			.expect( "Failed to instantiate plugin asynchronously" );
		let binding = Binding::new(
			bindings.root.package,
			HashMap::from([( bindings.root.name, bindings.root.spec )]),
			ExactlyOne( "_".to_string(), instance ),
		);

		match binding.dispatch( "root", "get-value", &[] ).await {
			Ok( ExactlyOne( _, Ok( Val::U32( 42 )))) => {}
			value => panic!( "Expected async dispatch to return U32(42), found: {:#?}", value ),
		}
	});
}
