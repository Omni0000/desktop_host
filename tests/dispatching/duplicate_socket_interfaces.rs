use std::collections::HashMap ;

use wasm_link::{ Binding, Engine, Linker };
use wasm_link::cardinality::ExactlyOne ;

fixtures! {
	bindings = { dependency: "dependency" };
	plugins  = { startup: "startup", child: "child" };
}

#[test]
fn duplicate_socket_interfaces_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
	let engine = Engine::default();
	let linker = Linker::new( &engine );
	let plugins = fixtures::plugins( &engine );
	let bindings = fixtures::bindings();
	let child = plugins.child.plugin.instantiate( &engine, &linker )?;
	let dependency = Binding::new(
		bindings.dependency.package,
		HashMap::from([( bindings.dependency.name, bindings.dependency.spec )]),
		ExactlyOne( "child".to_string(), child ),
	);
	let error = match plugins.startup.plugin.link(
		&engine,
		linker,
		vec![ dependency.clone(), dependency ],
	) {
		Err( error ) => error,
		Ok( _ ) => return Err( "duplicate synchronous sockets linked successfully".into() ),
	};
	assert_eq!( error.to_string(), "map entry `test:child/root` defined twice" );
	Ok(())
}

#[test]
fn duplicate_async_socket_interfaces_are_rejected_with_the_exact_error() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let engine = Engine::default();
		let linker = Linker::new( &engine );
		let plugins = fixtures::plugins( &engine );
		let bindings = fixtures::bindings();
		let child = plugins.child.plugin.instantiate( &engine, &linker )?;
		let dependency = Binding::new(
			bindings.dependency.package,
			HashMap::from([( bindings.dependency.name, bindings.dependency.spec )]),
			ExactlyOne( "child".to_string(), child ),
		);
		let error = match plugins.startup.plugin.link_async(
			&engine,
			linker,
			vec![ dependency.clone(), dependency ],
		).await {
			Err( error ) => error,
			Ok( _ ) => return Err( "duplicate asynchronous sockets linked successfully".into() ),
		};
		assert_eq!( error.to_string(), "map entry `test:child/root` defined twice" );
		Ok(())
	})
}
