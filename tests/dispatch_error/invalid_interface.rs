use std::collections::{ HashMap, HashSet };
use wasm_link::{ Binding, DispatchError, Engine, Function, FunctionKind, Interface, Linker, ReturnKind };
use wasm_link::cardinality::ExactlyOne ;

fixtures! {
	bindings = { root: "root" };
	plugins  = { test_plugin: "test-plugin" };
}

#[test]
fn async_dispatch_error_invalid_interface() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let engine = Engine::default();
		let linker = Linker::new( &engine );
		let plugins = fixtures::plugins( &engine );
		let bindings = fixtures::bindings();
		let plugin = plugins.test_plugin.plugin.instantiate_async(
			&engine,
			&linker,
		).await?;
		let binding = Binding::new(
			bindings.root.package,
			HashMap::from([( bindings.root.name, bindings.root.spec )]),
			ExactlyOne( "_".to_string(), plugin ),
		);

		assert!( matches!(
			binding.dispatch( "nonexistent", "test", &[] ).await,
			Err( DispatchError::InvalidInterfacePath( _ ))
		));
		Ok(())
	})
}

#[test]
fn plugin_dispatch_rejects_an_unexported_interface() -> Result<(), wasmtime::Error> {
	let engine = Engine::default();
	let linker = Linker::new( &engine );
	let plugins = fixtures::plugins( &engine );
	let plugin = plugins.test_plugin.plugin.instantiate( &engine, &linker )?;
	let binding = Binding::new(
		"test:dispatch-error",
		HashMap::from([(
			"missing".to_string(),
			Interface::new(
				HashMap::from([(
					"test".to_string(),
					Function::new( FunctionKind::Freestanding, ReturnKind::AssumeNoResources ),
				)]),
				HashSet::new(),
			),
		)]),
		ExactlyOne( "_".to_string(), plugin ),
	);

	assert!( matches!(
		binding.dispatch( "missing", "test", &[] ),
		Ok( ExactlyOne( _, Err( DispatchError::InvalidInterfacePath( _ ))))
	));
	Ok(())
}

#[test]
fn dispatch_error_invalid_interface() {

	let engine = Engine::default();
	let linker = Linker::new( &engine );
	let plugins = fixtures::plugins( &engine );
	let bindings = fixtures::bindings();

	let plugin_instance = plugins.test_plugin.plugin
		.instantiate( &engine, &linker )
		.expect( "Failed to instantiate plugin" );
	let binding = Binding::new(
		bindings.root.package,
		HashMap::from([( bindings.root.name, bindings.root.spec )]),
		ExactlyOne( "_".to_string(), plugin_instance ),
	);

	match binding.dispatch( "nonexistent", "test", &[] ) {
		Err( DispatchError::InvalidInterfacePath( _ )) => {}
		value => panic!( "Expected InvalidInterfacePath error, found: {:#?}", value ),
	}

}
