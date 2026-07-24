use std::collections::HashMap ;

use wasm_link::{ Binding, Engine, Linker, PluginInstanceAsync, PluginInstanceSync, SocketBindingAny, nem };
use wasm_link::cardinality::{ Any, AtLeastOne, AtMostOne, ExactlyOne };

use crate::fixture_linking::TestContext ;

fixtures! {
	bindings = { root: "root" };
	plugins  = { plugin: "plugin" };
}

#[test]
fn links_each_type_erased_binding_cardinality() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
	let engine = Engine::default();

	let bindings = fixtures::bindings();
	let instance = fixtures::plugins( &engine ).plugin.plugin
		.instantiate( &engine, &Linker::new( &engine ))?;
	let binding = Binding::new(
		bindings.root.package,
		HashMap::from([( bindings.root.name, bindings.root.spec )]),
		ExactlyOne( "plugin".to_string(), instance ),
	);
	let socket = binding.into_any();
	let socket_any: SocketBindingAny<String, TestContext> = socket.clone().into();
	let _socket_clone = socket_any.clone();
	let plugin = fixtures::plugins( &engine ).plugin.plugin;
	let _ = plugin.link( &engine, Linker::new( &engine ), vec![ socket.clone() ])?;
	let plugin = fixtures::plugins( &engine ).plugin.plugin;
	let _ = plugin.link_async( &engine, Linker::new( &engine ), vec![ socket ]).await?;

	let bindings = fixtures::bindings();
	let binding: Binding<String, TestContext, AtMostOne<String, PluginInstanceSync<TestContext>>> = Binding::new(
		bindings.root.package,
		HashMap::from([( bindings.root.name, bindings.root.spec )]),
		AtMostOne( None ),
	);
	let socket = binding.into_any();
	let plugin = fixtures::plugins( &engine ).plugin.plugin;
	let _ = plugin.link( &engine, Linker::new( &engine ), vec![ socket.clone() ])?;
	let plugin = fixtures::plugins( &engine ).plugin.plugin;
	let _ = plugin.link_async( &engine, Linker::new( &engine ), vec![ socket ]).await?;

	let bindings = fixtures::bindings();
	let instance = fixtures::plugins( &engine ).plugin.plugin
		.instantiate( &engine, &Linker::new( &engine ))?;
	let binding = Binding::new(
		bindings.root.package,
		HashMap::from([( bindings.root.name, bindings.root.spec )]),
		AtLeastOne( nem! { "plugin".to_string() => instance }),
	);
	let socket = binding.into_any();
	let plugin = fixtures::plugins( &engine ).plugin.plugin;
	let _ = plugin.link( &engine, Linker::new( &engine ), vec![ socket.clone() ])?;
	let plugin = fixtures::plugins( &engine ).plugin.plugin;
	let _ = plugin.link_async( &engine, Linker::new( &engine ), vec![ socket ]).await?;

	let bindings = fixtures::bindings();
	let binding: Binding<String, TestContext, Any<String, PluginInstanceSync<TestContext>>> = Binding::new(
		bindings.root.package,
		HashMap::from([( bindings.root.name, bindings.root.spec )]),
		Any( HashMap::new() ),
	);
	let socket = binding.into_any();
	let plugin = fixtures::plugins( &engine ).plugin.plugin;
	let _ = plugin.link( &engine, Linker::new( &engine ), vec![ socket.clone() ])?;
	let plugin = fixtures::plugins( &engine ).plugin.plugin;
	let _ = plugin.link_async( &engine, Linker::new( &engine ), vec![ socket ]).await?;
	Ok(())
	})
}

#[test]
fn links_each_async_type_erased_binding_cardinality() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let engine = Engine::default();
		let bindings = fixtures::bindings();
		let instance = fixtures::plugins( &engine ).plugin.plugin
			.instantiate_async( &engine, &Linker::new( &engine )).await?;
		let binding: Binding<
			String,
			TestContext,
			ExactlyOne<String, PluginInstanceAsync<TestContext>>,
			PluginInstanceAsync<TestContext>,
		> = Binding::new(
			bindings.root.package,
			HashMap::from([( bindings.root.name, bindings.root.spec )]),
			ExactlyOne( "plugin".to_string(), instance ),
		);
		let socket = binding.into_any();
		let socket_any: SocketBindingAny<String, TestContext> = socket.clone().into();
		let _socket_clone = socket_any.clone();
		let plugin = fixtures::plugins( &engine ).plugin.plugin;
		let _ = plugin.link_async(
			&engine,
			Linker::new( &engine ),
			vec![ socket ],
		).await?;

		let bindings = fixtures::bindings();
		let binding: Binding<
			String,
			TestContext,
			AtMostOne<String, PluginInstanceAsync<TestContext>>,
			PluginInstanceAsync<TestContext>,
		> = Binding::new(
			bindings.root.package,
			HashMap::from([( bindings.root.name, bindings.root.spec )]),
			AtMostOne( None ),
		);
		let socket = binding.into_any();
		let plugin = fixtures::plugins( &engine ).plugin.plugin;
		let _ = plugin.link_async(
			&engine,
			Linker::new( &engine ),
			vec![ socket ],
		).await?;

		let bindings = fixtures::bindings();
		let instance = fixtures::plugins( &engine ).plugin.plugin
			.instantiate_async( &engine, &Linker::new( &engine )).await?;
		let binding: Binding<
			String,
			TestContext,
			AtLeastOne<String, PluginInstanceAsync<TestContext>>,
			PluginInstanceAsync<TestContext>,
		> = Binding::new(
			bindings.root.package,
			HashMap::from([( bindings.root.name, bindings.root.spec )]),
			AtLeastOne( nem! { "plugin".to_string() => instance }),
		);
		let socket = binding.into_any();
		let plugin = fixtures::plugins( &engine ).plugin.plugin;
		let _ = plugin.link_async(
			&engine,
			Linker::new( &engine ),
			vec![ socket ],
		).await?;

		let bindings = fixtures::bindings();
		let binding: Binding<
			String,
			TestContext,
			Any<String, PluginInstanceAsync<TestContext>>,
			PluginInstanceAsync<TestContext>,
		> = Binding::new(
			bindings.root.package,
			HashMap::from([( bindings.root.name, bindings.root.spec )]),
			Any( HashMap::new() ),
		);
		let socket = binding.into_any();
		let plugin = fixtures::plugins( &engine ).plugin.plugin;
		let _ = plugin.link_async(
			&engine,
			Linker::new( &engine ),
			vec![ socket ],
		).await?;

		Ok(())
	})
}
