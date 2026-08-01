use std::collections::HashMap ;

use wasm_link::{ Binding, Engine, Linker, PluginInstanceAsync, PluginInstanceSync };
use wasm_link::cardinality::ExactlyOne ;

use crate::fixture_linking::TestContext ;

fixtures! {
	bindings = { root: "root" };
	plugins  = { plugin: "plugin" };
}

#[test]
fn debug_output_exposes_configuration_without_component_internals() -> Result<(), wasmtime::Error> {
	let engine = Engine::default();
	let linker = Linker::new( &engine );
	let plugins = fixtures::plugins( &engine );
	let bindings = fixtures::bindings();
	let plugin_debug = format!( "{:?}", plugins.plugin.plugin );
	assert!( plugin_debug.contains( "component: \"<Component>\"" ));
	assert!( plugin_debug.contains( "initial_fuel: None" ));
	assert!( plugin_debug.contains( "fuel_limiter: None" ));

	let plugin_instance = plugins.plugin.plugin.instantiate( &engine, &linker )?;
	let instance_debug = format!( "{plugin_instance:?}" );
	assert!( instance_debug.contains( "data: TestContext" ));
	assert!( instance_debug.contains( "fuel_limiter: None" ));
	let binding: Binding<
		String,
		TestContext,
		ExactlyOne<String, PluginInstanceSync<TestContext>>,
	> = Binding::new(
		bindings.root.package,
		HashMap::from([( bindings.root.name, bindings.root.spec )]),
		ExactlyOne( "plugin".to_string(), plugin_instance ),
	);
	let binding_debug = format!( "{binding:?}" );
	assert!( binding_debug.contains( "package_name: \"test:primitive\"" ));
	assert!( binding_debug.contains( "plugins: ExactlyOne" ));
	Ok(())
}

#[test]
fn async_debug_output_exposes_configuration_without_component_internals() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let engine = Engine::default();
		let linker = Linker::new( &engine );
		let plugins = fixtures::plugins( &engine );
		let bindings = fixtures::bindings();
		let plugin_instance = plugins.plugin.plugin.instantiate_async( &engine, &linker ).await?;
		let instance_debug = format!( "{plugin_instance:?}" );
		assert!( instance_debug.contains( "state: \"<session-managed async store>\"" ));
		let sync_instance = fixtures::plugins( &engine ).plugin.plugin.instantiate( &engine, &linker )?;
		let sync_instance = PluginInstanceAsync::from( sync_instance );
		assert!( format!( "{sync_instance:?}" ).contains( "state: \"<session-managed sync store>\"" ));
		let binding: Binding<
			String,
			TestContext,
			ExactlyOne<String, PluginInstanceAsync<TestContext>>,
			PluginInstanceAsync<TestContext>,
		> = Binding::new(
			bindings.root.package,
			HashMap::from([( bindings.root.name, bindings.root.spec )]),
			ExactlyOne( "plugin".to_string(), plugin_instance ),
		);
		let binding_debug = format!( "{binding:?}" );
		assert!( binding_debug.contains( "package_name: \"test:primitive\"" ));
		assert!( binding_debug.contains( "plugins: ExactlyOne" ));
		Ok(())
	})
}
