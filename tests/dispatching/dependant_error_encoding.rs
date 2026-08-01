use std::collections::HashMap ;

use wasm_link::{ Binding, Engine, Linker, Remap, Val };
use wasm_link::cardinality::ExactlyOne ;

fixtures! {
	bindings = { root: "root", dependency: "dependency" };
	plugins  = { startup: "startup", child: "child" };
}

#[test]
fn dependant_dispatch_encodes_child_errors() -> Result<(), Box<dyn std::error::Error>> {
	let engine = Engine::default();
	let linker = Linker::new( &engine );
	let plugins = fixtures::plugins( &engine );
	let bindings = fixtures::bindings();
	let child = plugins.child.plugin
		.remap_interfaces( HashMap::from([(
			"root".to_string(),
			Remap::found_as_with_item_resolution_table(
				"root",
				HashMap::from([( "get-value".to_string(), "missing".to_string() )]),
			),
		)]))
		.instantiate( &engine, &linker )?;
	let dependency = Binding::new(
		bindings.dependency.package,
		HashMap::from([( bindings.dependency.name, bindings.dependency.spec )]),
		ExactlyOne( "child".to_string(), child ),
	);
	let startup = plugins.startup.plugin.link( &engine, linker, vec![ dependency ])?;
	let root = Binding::new(
		bindings.root.package,
		HashMap::from([( bindings.root.name, bindings.root.spec )]),
		ExactlyOne( "startup".to_string(), startup ),
	);

	let result = root.dispatch( "root", "get-value", &[] )?;
	assert!( matches!(
		&result,
		ExactlyOne( _, Ok( Val::Tuple( items ))) if matches!( items.as_slice(),
			[ Val::String( id ), Val::Result( Err( Some( error ))) ] if
			id == "child"
			&& matches!( &**error, Val::Variant( name, Some( message )) if
				name == "invalid-function"
				&& matches!( &**message, Val::String( function ) if function.ends_with( ":missing" ))
			)
		)
	), "unexpected dispatch result: {result:#?}" );
	Ok(())
}

#[test]
fn async_dependant_dispatch_encodes_child_errors() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let engine = Engine::default();
		let linker = Linker::new( &engine );
		let plugins = fixtures::plugins( &engine );
		let bindings = fixtures::bindings();
		let child = plugins.child.plugin
			.remap_interfaces( HashMap::from([(
				"root".to_string(),
				Remap::found_as_with_item_resolution_table(
					"root",
					HashMap::from([( "get-value".to_string(), "missing".to_string() )]),
				),
			)]))
			.instantiate_async( &engine, &linker ).await?;
		let dependency = Binding::new(
			bindings.dependency.package,
			HashMap::from([( bindings.dependency.name, bindings.dependency.spec )]),
			ExactlyOne( "child".to_string(), child ),
		);
		let startup = plugins.startup.plugin.link_async(
			&engine,
			linker,
			vec![ dependency ],
		).await?;
		let root = Binding::new(
			bindings.root.package,
			HashMap::from([( bindings.root.name, bindings.root.spec )]),
			ExactlyOne( "startup".to_string(), startup ),
		);

		let result = root.dispatch( "root", "get-value", &[] ).await?;
		assert!( matches!(
			&result,
			ExactlyOne( _, Ok( Val::Tuple( items ))) if matches!( items.as_slice(),
				[ Val::String( id ), Val::Result( Err( Some( error ))) ] if
				id == "child"
				&& matches!( &**error, Val::Variant( name, Some( message )) if
					name == "invalid-function"
					&& matches!( &**message, Val::String( function ) if function.ends_with( ":missing" ))
				)
			)
		), "unexpected dispatch result: {result:#?}" );
		Ok(())
	})
}
