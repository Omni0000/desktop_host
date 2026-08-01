use std::collections::HashMap ;

use wasm_link::{ Binding, Engine, Linker, Val };
use wasm_link::cardinality::ExactlyOne ;

fixtures! {
	bindings = { root: "root", dependency: "dependency" };
	plugins  = { consumer: "consumer", counter: "counter" };
}

#[test]
fn native_async_resource_calls_route_to_the_owning_plugin() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let engine = Engine::default();
		let linker = Linker::new( &engine );
		let plugins = fixtures::plugins( &engine );
		let bindings = fixtures::bindings();
		let counter = plugins.counter.plugin
			.instantiate_async( &engine, &linker ).await?;
		let dependency = Binding::new(
			bindings.dependency.package,
			HashMap::from([( bindings.dependency.name, bindings.dependency.spec )]),
			ExactlyOne( "counter".to_string(), counter ),
		);
		let consumer = plugins.consumer.plugin.link_async(
			&engine,
			linker,
			vec![ dependency ],
		).await?;
		let root = Binding::new(
			bindings.root.package,
			HashMap::from([( bindings.root.name, bindings.root.spec )]),
			ExactlyOne( "consumer".to_string(), consumer ),
		);

		let result = root.dispatch( "root", "get-value", &[] ).await?;
		assert!( matches!( result, ExactlyOne( _, Ok( Val::U32( 42 )))));
		Ok(())
	})
}
