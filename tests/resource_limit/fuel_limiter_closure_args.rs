use std::collections::{ HashMap, HashSet };
use wasm_link::{ Binding, Engine, Function, FunctionKind, Interface, Linker, ReturnKind, Val };
use wasm_link::cardinality::ExactlyOne ;
use wasmtime::Config;

fixtures! {
	bindings = { root: "root" };
	plugins  = { burn_fuel: "burn-fuel" };
}

#[test]
fn closure_receives_correct_interface_and_function() {

	let mut config = Config::new();
	config.consume_fuel( true );
	let engine = Engine::new( &config ).expect( "failed to create engine" );
	let linker = Linker::new( &engine );
	let plugins = fixtures::plugins( &engine );
	let bindings = fixtures::bindings();

	let plugin_instance = plugins.burn_fuel.plugin
		.with_fuel_limiter(| _store, interface, function, _metadata | {
			assert_eq!( interface, "test:fuel/root" );
			assert_eq!( function, "burn" );
			100_000
		})
		.instantiate( &engine, &linker )
		.expect( "failed to instantiate plugin" );

	let binding = Binding::new(
		bindings.root.package,
		HashMap::from([( bindings.root.name, Interface::new(
			HashMap::from([( "burn".into(), Function::new( FunctionKind::Freestanding, ReturnKind::AssumeNoResources ))]),
			HashSet::new(),
		))]),
		ExactlyOne( "_".to_string(), plugin_instance ),
	);

	match binding.dispatch( "root", "burn", &[] ) {
		Ok( ExactlyOne( _, Ok( Val::U32( 42 )))) => {}
		other => panic!( "Expected Ok( U32( 42 )), got: {:#?}", other ),
	}
}

#[test]
fn async_instance_applies_the_per_call_fuel_limiter() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let mut config = Config::new();
		config.consume_fuel( true );
		let engine = Engine::new( &config )?;
		let linker = Linker::new( &engine );
		let plugins = fixtures::plugins( &engine );
		let bindings = fixtures::bindings();
		let plugin_instance = plugins.burn_fuel.plugin
			.with_fuel_limiter(| _store, interface, function, _metadata | {
				assert_eq!( interface, "test:fuel/root" );
				assert_eq!( function, "burn" );
				100_000
			})
			.instantiate_async( &engine, &linker ).await?;
		let binding = Binding::new(
			bindings.root.package,
			HashMap::from([( bindings.root.name, Interface::new(
				HashMap::from([( "burn".into(), Function::new( FunctionKind::Freestanding, ReturnKind::AssumeNoResources ))]),
				HashSet::new(),
			))]),
			ExactlyOne( "_".to_string(), plugin_instance ),
		);

		assert!( matches!(
			binding.dispatch( "root", "burn", &[] ).await?,
			ExactlyOne( _, Ok( Val::U32( 42 )))
		));
		Ok(())
	})
}
