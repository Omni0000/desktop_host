use std::collections::HashMap ;

use wasm_link::{ Binding, Engine, Linker, Val };
use wasm_link::cardinality::ExactlyOne ;

fixtures! {
	bindings = { root: "root" };
	plugins  = { plugin: "plugin" };
}

#[test]
fn dispatches_void_function() -> Result<(), Box<dyn std::error::Error>> {
	let engine = Engine::default();
	let linker = Linker::new( &engine );
	let plugins = fixtures::plugins( &engine );
	let bindings = fixtures::bindings();
	let instance = plugins.plugin.plugin.instantiate( &engine, &linker )?;
	let binding = Binding::new(
		bindings.root.package,
		HashMap::from([( bindings.root.name, bindings.root.spec )]),
		ExactlyOne( "plugin".to_string(), instance ),
	);

	assert!( matches!(
		binding.dispatch( "root", "run", &[] )?,
		ExactlyOne( _, Ok( Val::Option( None )))
	));
	Ok(())
}

#[test]
fn async_dispatches_void_function() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let engine = Engine::default();
		let linker = Linker::new( &engine );
		let plugins = fixtures::plugins( &engine );
		let bindings = fixtures::bindings();
		let instance = plugins.plugin.plugin.instantiate_async( &engine, &linker ).await?;
		let binding = Binding::new(
			bindings.root.package,
			HashMap::from([( bindings.root.name, bindings.root.spec )]),
			ExactlyOne( "plugin".to_string(), instance ),
		);

		assert!( matches!(
			binding.dispatch( "root", "run", &[] ).await?,
			ExactlyOne( _, Ok( Val::Option( None )))
		));
		Ok(())
	})
}
